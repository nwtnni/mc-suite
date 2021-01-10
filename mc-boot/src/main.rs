use std::mem;
use std::sync::Arc;
use std::time::Duration;

use rusoto_core::Region;
use rusoto_ec2::Ec2 as _;
use rusoto_ec2::Ec2Client;
use rusoto_ec2::InstanceStateChange;
use rusoto_ec2::StartInstancesRequest;
use rusoto_ec2::StopInstancesRequest;
use serenity::client;
use serenity::framework;
use serenity::model::id;
use serenity::model::voice;
use structopt::StructOpt;
use tokio::sync::mpsc;
use tokio::task;
use tokio::time;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(long, env = "DISCORD_TOKEN")]
    token: String,

    #[structopt(long, env = "DISCORD_GENERAL_CHANNEL_ID")]
    general_id: u64,

    #[structopt(long, env = "AWS_INSTANCE_ID")]
    instance_id: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {

    let opt = Opt::from_args();

    let (tx, mut rx) = mpsc::channel(10);

    let mut discord = serenity::Client::builder(&opt.token)
        .event_handler(Discord(tx))
        .framework(framework::StandardFramework::default())
        .await?;

    let general_channel = id::ChannelId::from(opt.general_id);
    let http = Arc::clone(&discord.cache_and_http);
    let ec2 = Ec2::new(Region::UsEast2, opt.instance_id);

    let mut connected = 0usize;
    let mut online = false;

    let discord = tokio::spawn(async move { discord.start().await });
    let main = tokio::spawn(async move {
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
                let typing = general_channel.start_typing(&http.http)?;
                let message = general_channel
                    .say(&http.http, "Server is starting...")
                    .await?;

                ec2.start().await?;

                general_channel
                    .edit_message(&http.http, message, |message| message.content("Server has started!"))
                    .await?;
                typing.stop();
            } else if connected == 0 && mem::replace(&mut online, false) {
                let typing = general_channel.start_typing(&http.http)?;
                let message = general_channel
                    .say(&http.http, "Server is stopping...")
                    .await?;

                ec2.hibernate().await?;

                general_channel
                    .edit_message(&http.http, message, |message| message.content("Server has stopped."))
                    .await?;
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
        _: Option<id::GuildId>,
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
    client: Arc<Ec2Client>,
    instance: String,
}

impl Ec2 {
    fn new(region: Region, instance: String) -> Self {
        Ec2 {
            client: Arc::new(Ec2Client::new(region)),
            instance,
        }
    }

    async fn start(&self) -> anyhow::Result<()> {
        let request = StartInstancesRequest {
            additional_info: None,
            dry_run: Some(false),
            instance_ids: vec![self.instance.clone()],
        };

        loop {
            let response = self.client.start_instances(request.clone()).await?;

            if self.success(response.starting_instances, RUNNING) {
                return Ok(());
            }

            time::sleep(SLEEP).await;
        }
    }

    async fn hibernate(&self) -> anyhow::Result<()> {
        let request = StopInstancesRequest {
            dry_run: Some(false),
            force: Some(false),
            hibernate: Some(true),
            instance_ids: vec![self.instance.clone()],
        };

        loop {
            let response = self.client.stop_instances(request.clone()).await?;

            if self.success(response.stopping_instances, STOPPED) {
                return Ok(());
            }

            time::sleep(SLEEP).await;
        }
    }

    fn success(&self, changes: Option<Vec<InstanceStateChange>>, state: i64) -> bool {
        changes
            .into_iter()
            .flatten()
            .filter(|change| change.instance_id.as_ref() == Some(&self.instance))
            .filter_map(|change| change.current_state)
            .filter_map(|state| state.code)
            .any(|code| code & 0b1111_1111 == state)
    }
}

impl Drop for Ec2 {
    fn drop(&mut self) {
        let ec2 = self.clone();
        task::spawn_blocking(|| async move { ec2.hibernate().await });
    }
}
