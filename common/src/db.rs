use sqlx::{postgres::PgPool, Pool, Postgres};

use std::env;

use anyhow::{Ok, Result};
use dotenv::dotenv;

use crate::{
    models::{Pnl, User, Wallet},
    utils::Currency,
};

pub async fn establish_connection() -> Pool<Postgres> {
    dotenv().ok();

    let db_url = env::var("DATABASE_URL").unwrap();
    println!("Db url: {:?} ", db_url);

    PgPool::connect(&db_url)
        .await
        .expect("Failed to create pool")
}

pub async fn get_user_wallet(
    pool: &Pool<Postgres>,
    user_id: i32,
    currency: Currency,
) -> anyhow::Result<Wallet> {
    let mut conn = pool
        .acquire()
        .await
        .expect("failed to get connection from the pool");

    let wallet: Wallet =
        sqlx::query_as("Select * from wallet where user_id = $1 and currency = $2")
            .bind(user_id)
            .bind(currency.to_string())
            .fetch_one(&mut conn)
            .await
            .expect("Failed to fetch wallet");

    Ok(wallet)
}

pub async fn update_user_wallet<'a>(
    pool: &'a Pool<Postgres>,
    user_id: i32,
    currency: Currency,
    new_balance: f64,
) -> anyhow::Result<()> {
    println!("Updating user wallet: {}", user_id);
    let mut conn = pool
        .acquire()
        .await
        .expect("failed to get connection from the pool");

    sqlx::query("UPDATE wallet SET balance = $1 WHERE user_id = $2 and currency = $3")
        .bind(new_balance)
        .bind(user_id)
        .bind(currency.to_string())
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");
    Ok(())
}

pub async fn create_user_and_update_tables<'a>(
    pool: &'a Pool<Postgres>,
    user: &User,
) -> anyhow::Result<()> {
    let mut conn = pool
        .acquire()
        .await
        .expect("Failed to get connections from the pool");

    sqlx::query("INSERT INTO wallet (user_id, currency, balance) VALUES ($1, $2, $3)")
        .bind(user.id) // Bind the user_id
        .bind(Currency::SOL.to_string()) // Set a default currency, e.g., USD
        .bind(0) // Initialize balance to deposit amount
        .execute(&mut conn)
        .await
        .expect("Error creating initial wallet");

    sqlx::query("INSERT INTO pnl (user_id, num_matches, profit) VALUES ($1, $2, $3)")
        .bind(user.id)
        .bind(0)
        .bind(0)
        .execute(&mut conn)
        .await?;
    Ok(())
}

pub async fn update_pnl<'a>(pool: &'a Pool<Postgres>, user_id: i32, profit: f64) -> Result<()> {
    let mut conn = pool.acquire().await.unwrap();

    let user_pnl: Pnl = sqlx::query_as("Select * from pnl where user_id = $1")
        .bind(user_id)
        .fetch_one(&mut conn)
        .await
        .expect("Failed to fetch user pnl");

    let pnl = user_pnl.profit + profit;
    let num_matches = user_pnl.num_matches + 1;
    sqlx::query("UPDATE pnl SET profit = $1, num_matches = $2 WHERE user_id = $3")
        .bind(pnl)
        .bind(num_matches)
        .bind(user_id)
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");

    Ok(())
}

pub async fn update_player_balances<'a>(
    pool: &'a Pool<Postgres>,
    user_ids: &[i32],
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
        let user_pnl: Pnl = sqlx::query_as("SELECT * FROM pnl WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        // Execute updates
        sqlx::query("UPDATE wallet SET balance = $1 WHERE user_id = $2 AND currency = $3")
            .bind(new_balance)
            .bind(user_id)
            .bind(Currency::SOL.to_string())
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE pnl SET profit = $1, num_matches = $2 WHERE user_id = $3")
            .bind(user_pnl.profit + profit)
            .bind(user_pnl.num_matches + 1)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}
