use std::fmt;

use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref JOIN: Regex = Regex::new(r".*\[Server thread/INFO\]: (.*)\[.*\] logged in with entity id .* at .*").unwrap();
    static ref QUIT: Regex = Regex::new(r".*\[Server thread/INFO\]: (.*) left the game").unwrap();
    static ref ACHIEVE: Regex = Regex::new(r".*\[Server thread/INFO\]: (.*) has made the advancement \[(.*)\]").unwrap();
    static ref MESSAGE: Regex = Regex::new(r".*\[Server thread/INFO\]: <([^ ]*)> (.*)").unwrap();
}

#[derive(Clone, Debug)]
pub enum Event<'line> {
    Join(&'line str),
    Quit(&'line str),
    Achieve(&'line str, &'line str),
    Message(&'line str, &'line str),
}

impl<'line> Event<'line> {
    pub fn parse(s: &str) -> Option<Event> {
        if let Some(cap) = JOIN.captures(s) {
            let name = cap.get(1).unwrap().as_str();
            Some(Event::Join(name))
        } else if let Some(cap) = QUIT.captures(s) {
            let name = cap.get(1).unwrap().as_str();
            Some(Event::Quit(name))
        } else if let Some(cap) = ACHIEVE.captures(s) {
            let name = cap.get(1).unwrap().as_str();
            let achieve = cap.get(2).unwrap().as_str();
            Some(Event::Achieve(name, achieve))
        } else if let Some(cap) = MESSAGE.captures(s) {
            let name = cap.get(1).unwrap().as_str();
            let message = cap.get(2).unwrap().as_str();
            Some(Event::Message(name, message))
        } else {
            None
        }
    }
}

impl<'line> fmt::Display for Event<'line> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use Event::*;
        match self {
        | Join(name) => write!(fmt, "{} has joined the server!", name),
        | Quit(name) => write!(fmt, "{} has left the server.", name),
        | Achieve(name, achieve) => write!(fmt, "{} has attained [{}].", name, achieve),
        | Message(name, message) => write!(fmt, "[{}]: {}", name, message),
        }
    }
}
