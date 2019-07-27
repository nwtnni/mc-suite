use std::io;
use std::process;
use std::sync;

use crate::event;
use crate::state;

use std::io::BufRead;

pub struct Server {
    general: discord::model::ChannelId,
    verbose: discord::model::ChannelId, 
    discord: sync::Arc<discord::Discord>,
    child: process::Child,
    rx: process::ChildStdout,
    state: sync::Arc<sync::Mutex<state::State>>,
}

impl Server {
    pub fn new(
        command: &str,
        general: discord::model::ChannelId,
        verbose: discord::model::ChannelId,
        discord: sync::Arc<discord::Discord>,
        state: sync::Arc<sync::Mutex<state::State>>,
    ) -> (
        sync::Arc<sync::Mutex<process::ChildStdin>>,
        Self,
    ) {
        let mut child = process::Command::new(command)
            .stdin(process::Stdio::piped())
            .stdout(process::Stdio::piped())
            .spawn()
            .expect("Failed to launch server");
        let rx = child.stdout.take()
            .expect("[IMPOSSIBLE]: stdout is piped");
        let tx = child.stdin.take()
            .expect("[IMPOSSIBLE]: stdin is piped");
        let tx = sync::Arc::new(sync::Mutex::new(tx));
        (tx, Server { general, verbose, discord, child, rx, state })
    }

    pub fn run(mut self) {
        let reader = io::BufReader::new(&mut self.rx);
        for line in reader.lines().map(Result::unwrap) {
            if let Some(event) = event::Event::parse(&line) {
                use event::Event::*;
                match &event {
                | Join(name) => self.state.lock().unwrap().insert_player(name),    
                | Quit(name) => self.state.lock().unwrap().remove_player(name),    
                | _ => (),
                };
                self.discord.send_message(self.general, &event.to_string(), "", false).ok();
            }
            self.discord.send_message(self.verbose, &line, "", false).ok();
            println!("{}", line);
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.wait().ok();
    }
}
