use std::io;
use std::process;
use std::sync;

use std::io::BufRead;
use std::io::Write;

use crate::event;

pub struct Server {
    channel: discord::model::ChannelId,
    discord: discord::Discord,
    child: process::Child,
    rx: process::ChildStdout,
    tx: sync::Arc<sync::Mutex<process::ChildStdin>>,
}

impl Server {
    pub fn new(
        command: &str,
        channel: discord::model::ChannelId,
        discord: discord::Discord
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
        (tx.clone(), Server { channel, discord, child, rx, tx })
    }

    pub fn run(mut self) {
        let reader = io::BufReader::new(&mut self.rx);
        for line in reader.lines().map(Result::unwrap) {
            if let Some(event) = event::Event::parse(&line) {
                self.discord.send_message(self.channel, &format!("{}", event), "", false).ok();
            }
            println!("{}", line);
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let stdin = self.child.stdin
            .as_mut()
            .expect("Parent always pipes stdin to child");
        writeln!(stdin, "stop").ok();
        self.child.wait().ok();
    }
}
