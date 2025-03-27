use anyhow::{Error, Result};
use sqlx::{postgres::PgPool, Pool, Postgres};
use std::env;
use tracing::info;

use crate::{
    models::{LeaderboardEntry, User, Wallet},
    utils::{Currency, Network},
};

pub async fn establish_connection() -> Pool<Postgres> {
    let db_url = env::var("DATABASE_URL").unwrap();
    info!("Db url: {:?} ", db_url);
    PgPool::connect(&db_url)
        .await
        .expect("Failed to create pool")
}

pub async fn get_user_wallet(
    pool: &Pool<Postgres>,
    user_id: i32,
    currency: Currency,
) -> Result<Wallet> {
    sqlx::query_as::<_, Wallet>("SELECT * FROM wallet WHERE user_id = $1 AND currency = $2")
        .bind(user_id)
        .bind(currency.to_string())
        .fetch_one(pool)
        .await
        .map_err(Error::from)
}

pub async fn update_user_wallet(
    pool: &Pool<Postgres>,
    user_id: i32,
    currency: Currency,
    new_balance: f64,
) -> Result<()> {
    info!("Updating user wallet: {}", user_id);
    sqlx::query(
        "UPDATE wallet SET balance = $1, updated_at = CURRENT_TIMESTAMP 
         WHERE user_id = $2 AND currency = $3",
    )
    .bind(new_balance)
    .bind(user_id)
    .bind(currency.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_user_and_wallet(
    pool: &Pool<Postgres>,
    user: &User,
    wallet_type: &str,
    wallet_address: Option<String>,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO wallet (user_id, currency, balance, wallet_type, wallet_address) 
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user.id)
    .bind(Currency::MON.to_string())
    .bind(0.0)
    .bind(wallet_type)
    .bind(wallet_address)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn update_player_balances(
    pool: &Pool<Postgres>,
    user_ids: &[i32],
    loser_idx: usize,
    single_bet_size: f64,
    winning_amount: f64,
    currency: Currency,
) -> Result<()> {
    info!("Updating player balances for user_ids: {:?}", user_ids);
    let mut tx = pool.begin().await?;
    // Default to SOLANA network if none is provided
    let currency_str = currency.to_string();

    for (i, user_id) in user_ids.iter().enumerate() {
        info!("Currency: {:?}, user_id: {:?}", currency_str, user_id);
        let current_balance: f64 =
            sqlx::query_scalar("SELECT balance FROM wallet WHERE user_id = $1 AND currency = $2")
                .bind(user_id)
                .bind(currency_str.clone())
                .fetch_one(&mut *tx)
                .await?;
        info!("Current balance: {:?}", current_balance);

        let (new_balance, profit) = if i == loser_idx {
            (current_balance - single_bet_size, -single_bet_size)
        } else {
            (current_balance + winning_amount, winning_amount)
        };

        sqlx::query(
            "UPDATE wallet SET balance = $1, updated_at = CURRENT_TIMESTAMP 
             WHERE user_id = $2 AND currency = $3",
        )
        .bind(new_balance)
        .bind(user_id)
        .bind(currency_str.clone())
        .execute(&mut *tx)
        .await?;

        record_game_result_tx(&mut tx, *user_id, &currency_str, profit).await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn record_game_result_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: i32,
    currency: &str,
    profit: f64,
) -> Result<(), Error> {
    info!(
        "Recording game result for user {} with profit {}",
        user_id, profit
    );
    info!("Currency: {:?}", currency);

    sqlx::query("INSERT INTO game_pnl (user_id, currency, profit) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(currency)
        .bind(profit)
        .execute(&mut **tx)
        .await?;

    sqlx::query(
        "INSERT INTO user_network_pnl (user_id, currency, total_matches, total_profit)
        VALUES ($1, $2, 1, $3)
        ON CONFLICT (user_id, currency) DO UPDATE
        SET total_matches = user_network_pnl.total_matches + 1,
            total_profit = user_network_pnl.total_profit + $3,
            updated_at = NOW()",
    )
    .bind(user_id)
    .bind(currency)
    .bind(profit)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn get_leaderboard_24h(
    pool: &Pool<Postgres>,
    currency: &str,
    limit: i32,
) -> Result<Vec<LeaderboardEntry>, Error> {
    sqlx::query_as("SELECT * FROM leaderboard_24h WHERE currency = $1 LIMIT $2")
        .bind(currency)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(Error::from)
}

pub async fn get_leaderboard_all_time(
    pool: &Pool<Postgres>,
    currency: &str,
    limit: i32,
) -> Result<Vec<LeaderboardEntry>, Error> {
    sqlx::query_as("SELECT * FROM leaderboard_all_time WHERE currency = $1 LIMIT $2")
        .bind(currency)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(Error::from)
}
