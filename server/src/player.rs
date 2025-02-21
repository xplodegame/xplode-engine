use serde::{Deserialize, Serialize};

// FIXME: If only id is a field element, try Player(String) instead
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
}

impl Player {
    pub fn new(id: String) -> Player {
        Player { id }
    }
}
