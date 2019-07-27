use std::env;
use std::error;
use std::io;
use std::time;
use std::thread;

use std::io::Write;
use std::io::BufRead;

mod disc;
mod event;
mod server;

fn main() -> Result<(), Box<dyn error::Error>> {

    let args = env::args().collect::<Vec<_>>();
    let token = env::var("DISCORD_TOKEN")?;
    let discord = discord::Discord::from_bot_token(&token)?;

    let (conn, _) = discord.connect()?;

    let general = env::var("GENERAL_CHANNEL")?
        .parse::<u64>()
        .map(discord::model::ChannelId)?;

    let verbose = env::var("VERBOSE_CHANNEL")?
        .parse::<u64>()
        .map(discord::model::ChannelId)?;

    let (tx, server) = server::Server::new(
        &args[1],
        general,
        verbose,
        discord,
    );
    let disc = disc::Disc::new(conn, tx.clone());

    thread::spawn(move || server.run());
    thread::spawn(move || disc.run());

    let stdin = io::stdin();
    for line in stdin.lock().lines().map(Result::unwrap) {
        writeln!(&mut tx.lock().unwrap(), "{}", line).ok();
        if line == "quit" {
            thread::sleep(time::Duration::from_secs(15));
            break
        }
    }

    Ok(())
}
