use anyhow::{Error, Result};
use sqlx::{postgres::PgPool, Pool, Postgres};
use std::env;
use tracing::info;

use crate::{
    models::{GamePnl, LeaderboardEntry, User, UserNetworkPnl, Wallet},
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
    .bind(Currency::SOL.to_string())
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
    network: Option<Network>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    // Default to SOLANA network if none is provided
    let network = network.unwrap_or(Network::SOLANA);
    let network_str = network.to_string();

    for (i, user_id) in user_ids.iter().enumerate() {
        let current_balance: f64 =
            sqlx::query_scalar("SELECT balance FROM wallet WHERE user_id = $1 AND currency = $2")
                .bind(user_id)
                .bind(Currency::SOL.to_string())
                .fetch_one(&mut *tx)
                .await?;

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
        .bind(Currency::SOL.to_string())
        .execute(&mut *tx)
        .await?;

        record_game_result_tx(&mut tx, *user_id, &network_str, profit).await?;
    }

    tx.commit().await?;
    Ok(())
}

async fn record_game_result_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_id: i32,
    network: &str,
    profit: f64,
) -> Result<()> {
    sqlx::query("INSERT INTO game_pnl (user_id, network, profit) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(network)
        .bind(profit)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO user_network_pnl (user_id, network, total_matches, total_profit)
        VALUES ($1, $2, 1, $3)
        ON CONFLICT (user_id, network) DO UPDATE
        SET total_matches = user_network_pnl.total_matches + 1,
            total_profit = user_network_pnl.total_profit + $3,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(user_id)
    .bind(network)
    .bind(profit)
    .execute(&mut *tx)
    .await?;

    Ok(())
}

pub async fn get_leaderboard_24h(
    pool: &PgPool,
    network: &str,
    limit: i64,
) -> Result<Vec<LeaderboardEntry>> {
    sqlx::query_as::<_, LeaderboardEntry>(
        "SELECT * FROM leaderboard_24h WHERE network = $1 LIMIT $2",
    )
    .bind(network)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(Error::from)
}

pub async fn get_leaderboard_all_time(
    pool: &PgPool,
    network: &str,
    limit: i64,
) -> Result<Vec<LeaderboardEntry>> {
    sqlx::query_as::<_, LeaderboardEntry>(
        "SELECT * FROM leaderboard_all_time WHERE network = $1 LIMIT $2",
    )
    .bind(network)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(Error::from)
}
