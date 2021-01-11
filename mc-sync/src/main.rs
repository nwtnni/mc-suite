use std::collections::HashSet;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::sync::Arc;

use joinery::JoinableIterator;
use once_cell::sync::Lazy;
use regex::Regex;
use serenity::client;
use serenity::framework;
use serenity::model::channel;
use serenity::model::id;
use structopt::StructOpt;
use tokio::io;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::net;
use tokio::process;
use tokio::sync::mpsc;

/// Shutdown port.
static PORT: u16 = 10101;
static ADDR: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Wrap a Minecraft server and synchronize the chat with Discord.
#[derive(Debug, StructOpt)]
struct Opt {
    /// Discord bot application token
    #[structopt(long, env = "DISCORD_TOKEN")]
    token: String,

    /// Forward interesting server events
    #[structopt(long, env = "DISCORD_GENERAL_CHANNEL_ID")]
    general_id: u64,

    /// Forward all server logs
    #[structopt(long, env = "DISCORD_VERBOSE_CHANNEL_ID")]
    verbose_id: u64,

    /// Shut down the computer after receiving the shutdown signal
    #[structopt(long)]
    shutdown: bool,

    /// Path to Minecraft server.jar or script
    command: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args();

    let (tx, mut rx) = mpsc::channel(10);

    let shutdown = Shutdown::new(tx.clone()).await?;
    let (mut child_stdin, mut child, minecraft) = Minecraft::new(&opt.command, tx.clone());
    let (mut stdout, stdin) = Stdin::new(tx.clone());
    let mut discord = serenity::Client::builder(&opt.token)
        .event_handler(Discord(tx))
        .framework(framework::StandardFramework::default())
        .await?;

    let http = Arc::clone(&discord.cache_and_http);
    let general_channel = id::ChannelId::from(opt.general_id);
    let verbose_channel = id::ChannelId::from(opt.verbose_id);
    let mut online = HashSet::<String>::new();

    tokio::spawn(async move { shutdown.start().await });
    tokio::spawn(async move { discord.start().await });
    tokio::spawn(async move { minecraft.start().await });
    tokio::spawn(async move { stdin.start().await });

    while let Some(event) = rx.recv().await {
        match event {
            Event::Discord(message) => {
                if message.author.name == "mc-sync" {
                    continue;
                }

                if message.content.trim() == "!online" {
                    let online =
                        format!("{} online: {}", online.len(), online.iter().join_with(", "));
                    message
                        .channel_id
                        .send_message(&http.http, |builder| builder.content(online))
                        .await?;
                    continue;
                }

                let say = format!("/say [{}]: {}\n", message.author.name, message.content);
                child_stdin.write_all(say.as_bytes()).await?;
                child_stdin.flush().await?;
            }
            Event::Minecraft(message) => {
                stdout.write_all(message.as_bytes()).await?;
                stdout.write_all(&[b'\n']).await?;
                stdout.flush().await?;

                verbose_channel
                    .send_message(&http.http, |builder| builder.content(&message))
                    .await?;

                let message = if let Some(captures) = JOIN.captures(&message) {
                    online.insert(captures[1].to_owned());
                    format!("{} joined the server!", &captures[1])
                } else if let Some(captures) = QUIT.captures(&message) {
                    online.remove(&captures[1]);
                    format!("{} left the server.", &captures[1])
                } else if let Some(captures) = ACHIEVEMENT.captures(&message) {
                    format!("{} unlocked achievement [{}]!", &captures[1], &captures[2])
                } else if let Some(captures) = MESSAGE.captures(&message) {
                    format!("[{}]: {}", &captures[1], &captures[2])
                } else {
                    continue;
                };

                general_channel
                    .send_message(&http.http, |builder| builder.content(&message))
                    .await?;
            }
            Event::Stdin(mut message) => {
                message.push('\n');
                child_stdin.write_all(message.as_bytes()).await?;
                child_stdin.flush().await?;
            }
            Event::Shutdown => {
                child_stdin.write_all(b"/stop\n").await?;
                child_stdin.flush().await?;
                child.wait().await?;
                break;
            }
        }
    }

    if opt.shutdown {
        process::Command::new("shutdown")
            .arg("now")
            .spawn()?
            .wait()
            .await?;
    }

    Ok(())
}

#[derive(Clone, Debug)]
enum Event {
    Discord(channel::Message),
    Minecraft(String),
    Stdin(String),
    Shutdown,
}

struct Discord(mpsc::Sender<Event>);

#[serenity::async_trait]
impl client::EventHandler for Discord {
    async fn message(&self, _: client::Context, message: channel::Message) {
        self.0
            .send(Event::Discord(message))
            .await
            .expect("[INTERNAL ERROR]: `rx` dropped");
    }
}

static JOIN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r".*\[Server thread/INFO\]: (.*)\[[^\]]*\] logged in with entity id .* at .*")
        .unwrap()
});

static QUIT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r".*\[Server thread/INFO\]: (.*) left the game").unwrap());

static ACHIEVEMENT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r".*\[Server thread/INFO\]: (.*) has made the advancement \[(.*)\]").unwrap()
});

static MESSAGE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r".*\[Server thread/INFO\]: <([^ \]]*)> (.*)").unwrap());

struct Minecraft {
    stdout: io::BufReader<process::ChildStdout>,
    tx: mpsc::Sender<Event>,
}

impl Minecraft {
    fn new(
        command: &str,
        tx: mpsc::Sender<Event>,
    ) -> (io::BufWriter<process::ChildStdin>, process::Child, Self) {
        let mut child = process::Command::new(command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("Failed to launch server");
        let stdout = child
            .stdout
            .take()
            .map(io::BufReader::new)
            .expect("[IMPOSSIBLE]: stdout is piped");
        let stdin = child
            .stdin
            .take()
            .map(io::BufWriter::new)
            .expect("[IMPOSSIBLE]: stdin is piped");
        (stdin, child, Minecraft { stdout, tx })
    }

    async fn start(self) -> anyhow::Result<()> {
        let mut lines = self.stdout.lines();
        while let Some(line) = lines.next_line().await? {
            self.tx.send(Event::Minecraft(line)).await?;
        }
        Ok(())
    }
}

struct Stdin {
    stdin: io::BufReader<io::Stdin>,
    tx: mpsc::Sender<Event>,
}

impl Stdin {
    fn new(tx: mpsc::Sender<Event>) -> (io::BufWriter<io::Stdout>, Self) {
        let stdin = io::BufReader::new(io::stdin());
        let stdout = io::BufWriter::new(io::stdout());
        (stdout, Stdin { stdin, tx })
    }

    async fn start(self) -> anyhow::Result<()> {
        let mut lines = self.stdin.lines();
        while let Some(line) = lines.next_line().await? {
            self.tx.send(Event::Stdin(line)).await?;
        }
        Ok(())
    }
}

struct Shutdown {
    listener: net::TcpListener,
    tx: mpsc::Sender<Event>,
}

impl Shutdown {
    async fn new(tx: mpsc::Sender<Event>) -> anyhow::Result<Self> {
        let listener = net::TcpListener::bind((ADDR, PORT)).await?;
        Ok(Self { listener, tx })
    }

    async fn start(self) -> anyhow::Result<()> {
        let (_, _) = self.listener.accept().await?;
        self.tx.send(Event::Shutdown).await?;
        Ok(())
    }
}
