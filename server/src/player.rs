use serde::{Deserialize, Serialize};

// FIXME: If only id is a field element, try Player(String) instead
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    pub name: String,
}

impl Player {
    pub fn new(id: String, name: String) -> Player {
        Player { id, name }
    }
}
