use std::borrow::Cow;
use std::process;
use std::sync::{Arc, Mutex};

use discord::model;
use joinery::prelude::*;

use crate::state;

use std::io::Write;

pub struct Disc {
    conn: discord::Connection,
    discord: Arc<discord::Discord>,
    general: discord::model::ChannelId,
    state: Arc<Mutex<state::State>>,
    tx: Arc<Mutex<process::ChildStdin>>,
}

impl Disc {
    pub fn new(
        conn: discord::Connection,
        discord: Arc<discord::Discord>,
        general: discord::model::ChannelId,
        state: Arc<Mutex<state::State>>,
        tx: Arc<Mutex<process::ChildStdin>>,
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
                    let state = self.state.lock().unwrap();
                    let count = state.online().len();
                    let names = state.online().iter().join_with(", ").to_string();
                    let message = if count == 0 {
                        Cow::from("Nobody is online.")
                    } else {
                        Cow::from(format!("{} online: {}", count, names))
                    };
                    self.discord.send_message(self.general, message.as_ref(), "", false).ok();
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
