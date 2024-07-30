use std::mem;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use rusoto_core::credential;
use rusoto_core::Region;
use rusoto_ec2::DescribeInstanceStatusRequest;
use rusoto_ec2::Ec2 as _;
use rusoto_ec2::Ec2Client;
use rusoto_ec2::StartInstancesRequest;
use serenity::all::GatewayIntents;
use serenity::client;
use serenity::model::id;
use serenity::model::voice;
use tokio::net;
use tokio::sync::mpsc;
use tokio::time;

/// Start and hibernate an EC2 instance based on Discord voice channel usage.
#[derive(Debug, Parser)]
struct Opt {
    /// Discord bot application token
    #[structopt(long, env = "DISCORD_TOKEN")]
    token: String,

    /// Send server status updates
    #[structopt(long, env = "DISCORD_GENERAL_CHANNEL_ID")]
    general_id: u64,

    /// AWS EC2 instance that the server runs on
    #[structopt(long, env = "AWS_INSTANCE_ID")]
    instance_id: String,

    /// AWS credential
    #[structopt(long, env = "AWS_ACCESS_KEY_ID")]
    access_key_id: String,

    /// AWS credential
    #[structopt(long, env = "AWS_SECRET_ACCESS_KEY")]
    secret_access_key: String,

    /// Minecraft server address
    #[structopt(long, env = "MINECRAFT_SERVER_URL")]
    server_url: String,

    /// Minecraft server shutdown port
    #[structopt(long, env = "MINECRAFT_SERVER_PORT")]
    server_port: u16,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let Opt {
        token,
        general_id,
        instance_id,
        access_key_id,
        secret_access_key,
        server_url,
        server_port,
    } = Opt::parse();

    let (tx, mut rx) = mpsc::channel(10);

    let mut discord = serenity::Client::builder(&token, GatewayIntents::GUILD_VOICE_STATES)
        .event_handler(Discord(tx))
        .await?;

    let general_channel = id::ChannelId::from(general_id);
    let http = Arc::clone(&discord.http);
    let ec2 = Ec2::new(
        Region::UsEast2,
        instance_id,
        access_key_id,
        secret_access_key,
    )?;

    let discord = tokio::spawn(async move { discord.start().await });
    let main = tokio::spawn(async move {
        let mut connected = 0usize;
        let mut online = false;

        while let Some(Event::Voice { old, new }) = rx.recv().await {
            match (old, new) {
                // Someone has joined a voice channel
                (
                    Some(voice::VoiceState {
                        channel_id: None, ..
                    }),
                    voice::VoiceState {
                        channel_id: Some(_),
                        ..
                    },
                )
                | (
                    None,
                    voice::VoiceState {
                        channel_id: Some(_),
                        ..
                    },
                ) => {
                    connected = connected.saturating_add(1);
                }
                // Someone has left a voice channel
                (
                    Some(voice::VoiceState {
                        channel_id: Some(_),
                        ..
                    }),
                    voice::VoiceState {
                        channel_id: None, ..
                    },
                ) => {
                    connected = connected.saturating_sub(1);
                }
                (_, _) => continue,
            }

            if connected > 0 && !mem::replace(&mut online, true) {
                let typing = general_channel.start_typing(&http);
                let message = general_channel.say(&http, "Server is starting...").await?;

                ec2.start().await?;

                message.delete(&http).await?;
                typing.stop();
            } else if connected == 0 && mem::replace(&mut online, false) {
                let typing = general_channel.start_typing(&http);
                let message = general_channel.say(&http, "Server is stopping...").await?;

                while let Err(error) = net::TcpStream::connect((&*server_url, server_port)).await {
                    eprintln!("{}", error);
                    time::sleep(SLEEP).await;
                }

                ec2.wait_until_stopped().await?;
                message.delete(&http).await?;
                typing.stop();
            }
        }
        Result::<_, anyhow::Error>::Ok(())
    });

    tokio::select! {
        result = discord => result??,
        result = main => result??,
    }

    Ok(())
}

#[derive(Clone, Debug)]
enum Event {
    Voice {
        old: Option<voice::VoiceState>,
        new: voice::VoiceState,
    },
}

struct Discord(mpsc::Sender<Event>);

#[serenity::async_trait]
impl client::EventHandler for Discord {
    async fn voice_state_update(
        &self,
        _: client::Context,
        old: Option<voice::VoiceState>,
        new: voice::VoiceState,
    ) {
        self.0
            .send(Event::Voice { old, new })
            .await
            .expect("[INTERNAL ERROR]: `rx` dropped");
    }
}

static SLEEP: Duration = Duration::from_secs(5);

// https://docs.rs/rusoto_ec2/0.46.0/rusoto_ec2/struct.InstanceState.html#structfield.code
static RUNNING: i64 = 16;
static STOPPED: i64 = 80;

#[derive(Clone)]
struct Ec2 {
    client: Ec2Client,
    instance_id: String,
}

impl Ec2 {
    fn new(
        region: Region,
        instance_id: String,
        access_key_id: String,
        secret_access_key: String,
    ) -> anyhow::Result<Self> {
        Ok(Ec2 {
            client: Ec2Client::new_with(
                rusoto_core::HttpClient::new()?,
                credential::StaticProvider::new_minimal(access_key_id, secret_access_key),
                region,
            ),
            instance_id,
        })
    }

    /// Start the instance and wait until it is running.
    async fn start(&self) -> anyhow::Result<()> {
        let request = StartInstancesRequest {
            additional_info: None,
            dry_run: Some(false),
            instance_ids: vec![self.instance_id.clone()],
        };

        while !self
            .client
            .start_instances(request.clone())
            .await?
            .starting_instances
            .into_iter()
            .flatten()
            .filter(|change| change.instance_id.as_ref() == Some(&self.instance_id))
            .filter_map(|change| change.current_state)
            .filter_map(|state| state.code)
            .any(|code| code & 0b1111_1111 == RUNNING)
        {
            time::sleep(SLEEP).await;
        }

        Ok(())
    }

    /// Wait until the instance is stopped.
    async fn wait_until_stopped(&self) -> anyhow::Result<()> {
        let request = DescribeInstanceStatusRequest {
            dry_run: Some(false),
            filters: None,
            include_all_instances: Some(true),
            instance_ids: Some(vec![self.instance_id.clone()]),
            max_results: None,
            next_token: None,
        };

        while !self
            .client
            .describe_instance_status(request.clone())
            .await?
            .instance_statuses
            .into_iter()
            .flatten()
            .filter(|status| status.instance_id.as_ref() == Some(&self.instance_id))
            .filter_map(|status| status.instance_state)
            .filter_map(|state| state.code)
            .any(|code| code & 0b1111_1111 == STOPPED)
        {
            time::sleep(SLEEP).await;
        }

        Ok(())
    }
}
