use serde::Deserialize;
use serde::Serialize;

#[derive(Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: i32,                         // Assuming id is INTEGER
    pub clerk_id: String,                // TEXT
    pub email: String,                   // TEXT
    pub name: String,                    // TEXT
    pub profile_picture: Option<String>, // TEXT, optional
    pub wallet_amount: i32,              // INTEGER
    pub created_at: Option<String>,      // TIMESTAMP, optional
    pub updated_at: Option<String>,      // TIMESTAMP, optional
}

#[derive(Deserialize, Serialize)]
pub struct Transaction {
    pub id: i32,                    // Assuming id is INTEGER
    pub user_id: i32,               // INTEGER
    pub amount: i32,                // INTEGER
    pub transaction_type: String,   // TEXT
    pub created_at: Option<String>, // TIMESTAMP, optional
}
