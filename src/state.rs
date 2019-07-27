use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct State {
    online: HashSet<String>,
}

impl State {
    pub fn insert_player(&mut self, player: &str) {
        if !self.online.contains(player) {
            self.online.insert(player.to_string());
        }
    }

    pub fn remove_player(&mut self, player: &str) {
        self.online.remove(player);
    }

    pub fn online(&self) -> &HashSet<String> {
        &self.online
    }
}
