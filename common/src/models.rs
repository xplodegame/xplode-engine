use chrono;
use serde::Deserialize;
use serde::Serialize;

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: i32,                                   // Assuming id is INTEGER
    pub privy_id: String,                          // TEXT, optional
    pub email: String,                             // TEXT
    pub name: String,                              // TEXT
    pub user_pda: Option<String>,                  // Changed to Option<String>
    pub created_at: chrono::DateTime<chrono::Utc>, // Use proper timestamp type
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub gif_ids: Vec<i32>,
}

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Wallet {
    pub id: i32,
    pub user_id: i32,
    pub currency: String,
    pub balance: f64,
    pub wallet_type: String,
    pub wallet_address: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct Transaction {
    pub id: i32,
    pub user_id: i32,
    pub wallet_id: i32,
    pub amount: f64,
    pub currency: String,
    pub tx_type: String,
    pub tx_hash: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct GamePnl {
    pub id: i32,
    pub user_id: i32,
    pub currency: String,
    pub profit: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct UserNetworkPnl {
    pub id: i32,
    pub user_id: i32,
    pub currency: String,
    pub total_matches: i32,
    pub total_profit: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct LeaderboardEntry {
    pub name: String,
    pub currency: String,
    pub total_profit: f64,
    pub total_matches: i64,
    pub rank: i64,
}
