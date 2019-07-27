use std::process;
use std::sync;

use discord::model;
use joinery::prelude::*;

use crate::state;

use std::io::Write;

pub struct Disc {
    conn: discord::Connection,
    discord: sync::Arc<discord::Discord>,
    general: discord::model::ChannelId,
    state: sync::Arc<sync::Mutex<state::State>>,
    tx: sync::Arc<sync::Mutex<process::ChildStdin>>,
}

impl Disc {
    pub fn new(
        conn: discord::Connection,
        discord: sync::Arc<discord::Discord>,
        general: discord::model::ChannelId,
        state: sync::Arc<sync::Mutex<state::State>>,
        tx: sync::Arc<sync::Mutex<process::ChildStdin>>,
    ) -> Self {
        Disc { conn, discord, general, state, tx } 
    }

    pub fn run(mut self) {
        loop {
            use model::Event::*;
            match self.conn.recv_event() {
            | Ok(MessageCreate(ref message)) if &message.author.name != "mc-sync" => {
                match message.content.as_ref() {
                | "!online" => {
                    let online = self.state.lock()
                        .unwrap()
                        .online()
                        .iter()
                        .join_with(", ")
                        .to_string();
                    self.discord.send_message(self.general, &online, "", false).ok();
                }
                | _ => {
                    writeln!(
                        &mut self.tx.lock().unwrap(),
                        "/say [{}]: {}",
                        message.author.name,
                        message.content
                    ).ok();
                }
                }
            }
            | _ => (),
            }
        }
    }
}
