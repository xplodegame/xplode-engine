use std::env;

use anyhow::{Ok, Result};
use dotenv::dotenv;
use sqlx::{pool, Executor, Pool, SqlitePool};

use crate::{
    models::{Pnl, User, Wallet},
    utils::{Currency, UserDetailsRequest},
};

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

pub async fn update_user_wallet<'a>(
    pool: &'a Pool<sqlx::Sqlite>,
    user_id: u32,
    currency: Currency,
    new_balance: f64,
) -> anyhow::Result<()> {
    println!("Updating user wallet: {}", user_id);
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

pub async fn create_user_and_update_tables<'a>(
    pool: &'a Pool<sqlx::Sqlite>,
    user: &User,
) -> anyhow::Result<()> {
    let mut conn = pool
        .acquire()
        .await
        .expect("Failed to get connections from the pool");

    sqlx::query("INSERT INTO wallet (user_id, currency, balance) VALUES (?, ?, ?)")
        .bind(user.id) // Bind the user_id
        .bind(Currency::SOL.to_string()) // Set a default currency, e.g., USD
        .bind(0) // Initialize balance to deposit amount
        .execute(&mut conn)
        .await
        .expect("Error creating initial wallet");

    sqlx::query("INSERT INTO pnl (user_id, num_matches, profit) VALUES (?, ?, ?) ")
        .bind(user.id)
        .bind(0)
        .bind(0)
        .execute(&mut conn)
        .await?;
    Ok(())
}

pub async fn update_pnl<'a>(pool: &'a Pool<sqlx::Sqlite>, user_id: u32, profit: f64) -> Result<()> {
    let mut conn = pool.acquire().await.unwrap();

    let user_pnl: Pnl = sqlx::query_as("Select * from pnl where user_id = ? ")
        .bind(user_id)
        .fetch_one(&mut conn)
        .await
        .expect("Failed to fetch user pnl");

    let pnl = user_pnl.profit + profit;
    let num_matches = user_pnl.num_matches + 1;
    sqlx::query("UPDATE pnl SET profit = ?, num_matches = ? WHERE user_id = ?")
        .bind(pnl)
        .bind(num_matches)
        .bind(user_id)
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");

    Ok(())
}

pub async fn update_player_balances<'a>(
    pool: &'a Pool<sqlx::Sqlite>,
    user_ids: &[u32],
    loser_idx: usize,
    single_bet_size: f64,
    winning_amount: f64,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    for (i, user_id) in user_ids.iter().enumerate() {
        let (new_balance, profit) = if i == loser_idx {
            (
                get_user_wallet(&pool, *user_id, Currency::SOL)
                    .await?
                    .balance
                    - single_bet_size,
                -single_bet_size,
            )
        } else {
            (
                get_user_wallet(&pool, *user_id, Currency::SOL)
                    .await?
                    .balance
                    + winning_amount,
                winning_amount,
            )
        };

        // Fetch existing PNL
        let user_pnl: Pnl = sqlx::query_as("SELECT * FROM pnl WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        // Execute updates
        sqlx::query("UPDATE wallet SET balance = ? WHERE user_id = ? AND currency = ?")
            .bind(new_balance)
            .bind(user_id)
            .bind(Currency::SOL.to_string())
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE pnl SET profit = ?, num_matches = ? WHERE user_id = ?")
            .bind(user_pnl.profit + profit)
            .bind(user_pnl.num_matches + 1)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}
