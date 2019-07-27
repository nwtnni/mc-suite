use std::fmt;

use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref JOIN: Regex = Regex::new(r"(.*)\[.*\] logged in with entity id at .*").unwrap();
    static ref QUIT: Regex = Regex::new(r"(.*) left the game").unwrap();
    static ref ACHIEVE: Regex = Regex::new(r"(.*) has made the advancement \[(.*)\]").unwrap();
    static ref MESSAGE: Regex = Regex::new(r"<([^ ]*)> (.*)").unwrap();
}

#[derive(Clone, Debug)]
pub enum Event {
    Join(String),
    Quit(String),
    Achieve(String, String),
    Message(String, String),
}

impl Event {
    pub fn parse(s: &str) -> Option<Event> {
        if let Some(cap) = JOIN.captures(s) {
            Some(Event::Join(cap[1].to_string()))
        } else if let Some(cap) = QUIT.captures(s) {
            Some(Event::Quit(cap[1].to_string()))
        } else if let Some(cap) = ACHIEVE.captures(s) {
            Some(Event::Achieve(cap[1].to_string(), cap[2].to_string()))
        } else if let Some(cap) = MESSAGE.captures(s) {
            Some(Event::Message(cap[1].to_string(), cap[2].to_string()))
        } else {
            None
        }
    }
}

impl fmt::Display for Event {
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
