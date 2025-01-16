use std::env;

use anyhow::Ok;
use dotenv::dotenv;
use sqlx::{Pool, SqlitePool};

use crate::{models::Wallet, utils::Currency};

pub async fn establish_connection() -> Pool<sqlx::Sqlite> {
    dotenv().ok();

    let db_url = env::var("DATABASE_URL").unwrap();
    println!("Db url: {:?} ", db_url);

    SqlitePool::connect(&db_url)
        .await
        .expect("Failed to create pool")
}
pub async fn get_user_wallet(
    pool: &Pool<sqlx::Sqlite>,
    user_id: u32,
    currency: Currency,
) -> anyhow::Result<Wallet> {
    let mut conn = pool
        .acquire()
        .await
        .expect("failed to get connection from the pool");

    let wallet: Wallet = sqlx::query_as("Select * from wallet where user_id = ? and currency = ?")
        .bind(user_id)
        .bind(currency.to_string())
        .fetch_one(&mut conn)
        .await
        .expect("Failed to fetch wallet");

    Ok(wallet)
}

pub async fn update_user_wallet(
    pool: &Pool<sqlx::Sqlite>,
    user_id: u32,
    currency: Currency,
    new_balance: f64,
) -> anyhow::Result<()> {
    let mut conn = pool
        .acquire()
        .await
        .expect("failed to get connection from the pool");

    sqlx::query("UPDATE wallet SET balance = ? WHERE user_id = ? and currency = ?")
        .bind(new_balance)
        .bind(user_id)
        .bind(currency.to_string())
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");
    Ok(())
}
