use chrono;
use serde::Deserialize;
use serde::Serialize;

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: i32,                                   // Assuming id is INTEGER
    pub clerk_id: String,                          // TEXT, optional
    pub email: String,                             // TEXT
    pub name: String,                              // TEXT
    pub user_pda: String,                          //TEXT
    pub created_at: chrono::DateTime<chrono::Utc>, // Use proper timestamp type
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Wallet {
    pub id: i32,
    pub user_id: i32,
    pub currency: String,
    pub balance: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize, Serialize)]
pub struct Transaction {
    pub id: i32,
    pub user_id: i32,
    pub amount: f64,
    pub tx_type: String,
    pub tx_hash: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct Pnl {
    pub id: i32,
    pub user_id: i32,
    pub num_matches: i32,
    pub profit: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
