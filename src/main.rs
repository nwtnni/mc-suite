use std::env;
use std::error;
use std::io;

use std::io::BufRead;

fn main() -> Result<(), Box<dyn error::Error>> {

    let token = env::var("TOKEN")?;
    let discord = discord::Discord::from_bot_token(&token)?;
    let stdin = io::stdin();

    let info = regex::Regex::new(r".*\[Server thread/INFO\]: (.*)")?;

    for line in stdin.lock().lines().map(Result::unwrap) {

        println!("{}", line);

        if let Some(cap) = info.captures(&line) {
            println!("{}", &cap[0]);
        }
    }

    Ok(())
}
