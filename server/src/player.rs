use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    funds: u64,
}

impl Player {
    pub fn new(id: String) -> Player {
        Player { id, funds: 100_u64 }
    }

    // TODO remove this function
    pub fn add_funds(&mut self) {
        self.funds += 100;
    }
}
