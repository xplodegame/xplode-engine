use std::env;

use dotenv::dotenv;
use sqlx::{Pool, SqlitePool};

pub async fn establish_connection() -> Pool<sqlx::Sqlite> {
    dotenv().ok();

    let db_url = env::var("DATABASE_URL").unwrap();

    SqlitePool::connect(&db_url)
        .await
        .expect("Failed to create pool")
}

pub async fn update_user(
    pool: &Pool<sqlx::Sqlite>,
    user_id: i32,
    new_balance: i32,
) -> anyhow::Result<()> {
    let mut conn = pool
        .acquire()
        .await
        .expect("failed to get connection from the pool");

    // Update the user's wallet balance
    sqlx::query("UPDATE users SET wallet_amount = ? WHERE id = ?")
        .bind(new_balance)
        .bind(user_id)
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");

    Ok(())
}
