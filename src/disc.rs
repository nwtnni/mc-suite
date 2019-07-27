use std::process;
use std::sync;

use discord::model;

use std::io::Write;

pub struct Disc {
    conn: discord::Connection,
    tx: sync::Arc<sync::Mutex<process::ChildStdin>>,
}

impl Disc {
    pub fn new(
        conn: discord::Connection,
        tx: sync::Arc<sync::Mutex<process::ChildStdin>>,
    ) -> Self {
        Disc { conn, tx } 
    }

    pub fn run(mut self) {
        loop {
            use model::Event::*;
            match self.conn.recv_event() {
            | Ok(MessageCreate(ref message)) if &message.author.name != "mc-sync" => {
                writeln!(
                    &mut self.tx.lock().unwrap(),
                    "/say [{}]: {}",
                    message.author.name,
                    message.content
                ).ok();
            }
            | _ => (),
            }
        }
    }
}
