use std::env;
use std::error;
use std::io;

use std::io::Write;
use std::str::FromStr;

use std::io::BufRead;

trait Tap: Sized {
    fn tap<T, F: FnOnce(Self) -> T>(self, f: F) -> T {
        f(self) 
    }
}

impl<T: Sized> Tap for T {}

fn main() -> Result<(), Box<dyn error::Error>> {

    let discord = env::var("TOKEN")?
        .as_str()
        .tap(discord::Discord::from_bot_token)?;

    let server = env::var("SERVER")?
        .as_str()
        .tap(u64::from_str)?
        .tap(discord::model::ServerId);

    let channel = env::var("CHANNEL")?
        .as_str()
        .tap(u64::from_str)?
        .tap(discord::model::ChannelId);

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let info = regex::Regex::new(r".*\[Server thread/INFO\]: (.*)")?;

    for line in stdin.lock().lines().map(Result::unwrap) {

        writeln!(out, "{}", line)?;

        if let Some(cap) = info.captures(&line) {
            writeln!(out, "{}", &cap[0])?;
        }
    }

    Ok(())
}
