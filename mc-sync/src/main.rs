use std::collections::HashSet;
use std::future::IntoFuture as _;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::LazyLock;

use clap::Parser;
use joinery::JoinableIterator;
use regex::Regex;
use serenity::all::GatewayIntents;
use serenity::client;
use serenity::model::channel;
use serenity::model::id;
use tokio::io;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::net;
use tokio::process;
use tokio::runtime;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

/// Wrap a Minecraft server and synchronize the chat with Discord.
#[derive(Debug, Parser)]
struct Opt {
    /// Discord bot application token
    #[arg(long, env = "DISCORD_TOKEN")]
    token: String,

    /// Forward interesting server events
    #[arg(long, env = "DISCORD_GENERAL_CHANNEL_ID")]
    general_id: u64,

    /// Forward all server logs
    #[arg(long, env = "DISCORD_VERBOSE_CHANNEL_ID")]
    verbose_id: u64,

    /// Shutdown port
    #[arg(long, env = "MINECRAFT_SERVER_PORT")]
    server_port: u16,

    /// Path to Minecraft server.jar or script
    command: String,
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    let runtime = runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let _guard = runtime.enter();

    let (event_tx, event_rx) = mpsc::channel(10);

    let shutdown = runtime.block_on(Shutdown::new(opt.server_port))?;
    let (child_stdin, mut child, minecraft) = Minecraft::new(&opt.command, event_tx.clone());
    let (stdout, stdin) = Stdin::new(event_tx.clone());
    let mut discord = runtime.block_on(
        serenity::Client::builder(
            &opt.token,
            GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT,
        )
        .event_handler(Discord(event_tx))
        .into_future(),
    )?;

    let http = Arc::clone(&discord.http);
    let general_channel = id::ChannelId::from(opt.general_id);
    let verbose_channel = id::ChannelId::from(opt.verbose_id);

    // If any long-running task returns or errors unexpectedly, try to shut down
    // the Minecraft server gracefully.
    runtime.spawn(async move {
        let child_stdin = Mutex::new(child_stdin);

        let finished = tokio::select! {
            finished = shutdown.start() => finished,
            finished = discord.start() => finished.map_err(anyhow::Error::from),
            finished = minecraft.start() => finished,
            finished = stdin.start() => finished,
            finished = process(
                event_rx,
                &child_stdin,
                stdout,
                http,
                general_channel,
                verbose_channel,
            ) => finished,
        };

        let mut child_stdin = child_stdin.lock().await;
        child_stdin.write_all(b"/stop\n").await?;
        child_stdin.flush().await?;
        finished
    });

    runtime.block_on(child.wait())?;
    runtime.shutdown_background();

    Ok(())
}

async fn process(
    mut event_rx: mpsc::Receiver<Event>,
    child_stdin: &Mutex<io::BufWriter<process::ChildStdin>>,
    mut stdout: io::BufWriter<io::Stdout>,
    http: Arc<serenity::http::Http>,
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
                    message.channel_id.say(&http, online).await?;
                    continue;
                }

                let say = format!("/say [{}]: {}\n", message.author.name, message.content);
                let mut child_stdin = child_stdin.lock().await;
                child_stdin.write_all(say.as_bytes()).await?;
                child_stdin.flush().await?;
            }
            Event::Minecraft(message) => {
                stdout.write_all(message.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;

                verbose_channel.say(&http, &message).await?;

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

                general_channel.say(&http, message).await?;
            }
            Event::Stdin(message) => {
                let mut child_stdin = child_stdin.lock().await;
                child_stdin.write_all(message.as_bytes()).await?;
                child_stdin.write_all(b"\n").await?;
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

static JOIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r".*\[Server thread/INFO\]: (.*)\[[^\]]*\] logged in with entity id .* at .*")
        .unwrap()
});

static QUIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r".*\[Server thread/INFO\]: (.*) left the game").unwrap());

static ACHIEVEMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r".*\[Server thread/INFO\]: (.*) has made the advancement \[(.*)\]").unwrap()
});

static MESSAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r".*\[Server thread/INFO\]: <([^ \]]*)> (.*)").unwrap());

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

struct Shutdown(net::TcpListener);

impl Shutdown {
    async fn new(port: u16) -> anyhow::Result<Self> {
        net::TcpListener::bind((IpAddr::V4(Ipv4Addr::UNSPECIFIED), port))
            .await
            .map(Self)
            .map_err(anyhow::Error::from)
    }

    async fn start(self) -> anyhow::Result<()> {
        let (_, _) = self.0.accept().await?;
        Ok(())
    }
}
