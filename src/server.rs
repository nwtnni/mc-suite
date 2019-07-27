use std::process;

use std::io::Write;

pub struct Server {
    child: process::Child,
}

impl Server {
    pub fn new(command: &str) -> Self {
        let child = process::Command::new(command)
            .stdin(process::Stdio::piped())
            .stdout(process::Stdio::piped())
            .spawn()
            .expect("Failed to launch server");
        Server { child }
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
