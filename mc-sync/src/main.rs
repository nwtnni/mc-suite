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
use tokio::runtime;
use tokio::sync::mpsc;

/// Shutdown port.
static PORT: u16 = 10101;
static ADDR: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
static STOP: &str = "/stop";

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

    /// Path to Minecraft server.jar or script
    command: String,
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args();

    let runtime = runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let _guard = runtime.enter();

    let (event_tx, event_rx) = mpsc::channel(10);

    let shutdown = runtime.block_on(Shutdown::new(event_tx.clone()))?;
    let (child_stdin, mut child, minecraft) = Minecraft::new(&opt.command, event_tx.clone());
    let (stdout, stdin) = Stdin::new(event_tx.clone());
    let mut discord = runtime.block_on({
        serenity::Client::builder(&opt.token)
            .event_handler(Discord(event_tx))
            .framework(framework::StandardFramework::default())
    })?;

    let http = Arc::clone(&discord.cache_and_http);
    let general_channel = id::ChannelId::from(opt.general_id);
    let verbose_channel = id::ChannelId::from(opt.verbose_id);

    runtime.spawn(async move { shutdown.start().await });
    runtime.spawn(async move { discord.start().await });
    runtime.spawn(async move { minecraft.start().await });
    runtime.spawn(async move { stdin.start().await });
    runtime.spawn(async move {
        process(
            event_rx,
            child_stdin,
            stdout,
            http,
            general_channel,
            verbose_channel,
        )
        .await
    });

    runtime.block_on(child.wait())?;
    runtime.shutdown_background();

    Ok(())
}

async fn process(
    mut event_rx: mpsc::Receiver<Event>,
    mut child_stdin: io::BufWriter<process::ChildStdin>,
    mut stdout: io::BufWriter<io::Stdout>,
    http: Arc<serenity::CacheAndHttp>,
    general_channel: id::ChannelId,
    verbose_channel: id::ChannelId,
) -> anyhow::Result<()> {
    let mut online = HashSet::<String>::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::Discord(message) => {
                if message.author.name == "mc-boot" || message.author.name == "mc-sync" {
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
            Event::Stdin(message) => {
                child_stdin.write_all(message.as_bytes()).await?;
                child_stdin.write_all(&[b'\n']).await?;
                child_stdin.flush().await?;
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
enum Event {
    Discord(channel::Message),
    Minecraft(String),
    Stdin(String),
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
        self.tx.send(Event::Stdin(String::from(STOP))).await?;
        Ok(())
    }
}
