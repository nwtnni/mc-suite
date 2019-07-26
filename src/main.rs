use std::env;
use std::error;
use std::io;

use std::io::Write;
use std::io::BufRead;

fn main() -> Result<(), Box<dyn error::Error>> {

    let token = env::var("TOKEN")?;
    let discord = discord::Discord::from_bot_token(&token)?;

    discord.connect()?;

    let channel = env::var("CHANNEL")?
        .parse::<u64>()
        .map(discord::model::ChannelId)?;

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let info = regex::Regex::new(r".*\[Server thread/INFO\]: (.*)")?;

    for line in stdin.lock().lines().map(Result::unwrap) {

        writeln!(out, "{}", line)?;

        if let Some(cap) = info.captures(&line) {
            discord.send_message(channel, &cap[1], "", false)?;
        }
    }

    Ok(())
}
