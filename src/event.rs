#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Player(symbol::Symbol);

#[derive(Clone, Debug)]
pub enum Event {
    Join(Player),
    Quit(Player),
    Die(Player, String),
    Message(Player, String),
    Achieve(Player, String),
}
