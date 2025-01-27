use serde::Deserialize;
use serde::Serialize;

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: u32,            // Assuming id is INTEGER
    pub clerk_id: String,   // TEXT, optional
    pub email: String,      // TEXT
    pub name: String,       // TEXT
    pub user_pda: String,   //TEXT
    pub created_at: String, // TIMESTAMP, optional
    pub updated_at: String, // TIMESTAMP, optional
}

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Wallet {
    pub id: u32,            // Assuming id is INTEGER
    pub user_id: u32,       // Foreign key to User
    pub currency: String,   // Currency type
    pub balance: f64,       // Balance as a decimal
    pub created_at: String, // TIMESTAMP, optional
    pub updated_at: String, // TIMESTAMP, optional
}

#[derive(Deserialize, Serialize)]
pub struct Transaction {
    pub id: u32,         // Assuming id is INTEGER
    pub user_id: u32,    // Foreign key to User
    pub amount: f64,     // Amount as a decimal
    pub tx_type: String, // TEXT
    pub tx_hash: String,
    pub created_at: Option<String>, // TIMESTAMP, optional
}
