use std::{env, fs, time::SystemTime};

use actix_cors::Cors;
use actix_web::{middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use common::{
    db,
    models::{self, LeaderboardEntry, User, UserNetworkPnl, Wallet},
    utils::{
        self, Currency, DepositRequest, Network, UpdateUserDetailsRequest, UserDetailsRequest,
        WalletType, WithdrawRequest,
    },
};
use db::establish_connection;
use dotenv::dotenv;

use evm_deposits::transfer_funds;
use serde_json::json;
use sqlx::{Pool, Postgres};
use tracing::info;
use tracing_subscriber::EnvFilter;
use utils::TxType;

#[actix_web::post("/user-details")]
async fn fetch_or_create_user(
    req: web::Json<UserDetailsRequest>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    info!(
        "Fetching or creating user with privy_id: {:?}",
        req.privy_id
    );
    let AppState { pool } = &**app_state;
    let mut tx = pool.begin().await.expect("Failed to start transaction");

    // Check if the user already exists
    let existing_user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE privy_id = $1")
        .bind(&req.privy_id)
        .fetch_optional(&mut *tx)
        .await
        .expect("Error fetching user");

    match existing_user {
        Some(user) => {
            let wallet: Wallet =
                sqlx::query_as("SELECT * FROM wallet WHERE user_id = $1 AND currency = $2")
                    .bind(user.id)
                    .bind(Currency::MON.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .expect("Error fetching wallet");

            tx.commit().await.expect("Failed to commit transaction");

            HttpResponse::Ok().json(json!({
                "id": user.id,
                "currency": Currency::MON.to_string(),
                "name": user.name,
                "balance": wallet.balance,
                "wallet_type": wallet.wallet_type,
                "wallet_address": wallet.wallet_address.unwrap_or_else(|| "".to_string())
            }))
        }
        None => {
            // Create new user
            let created_user: User = sqlx::query_as(
                "INSERT INTO users (privy_id, email, name) VALUES ($1, $2, $3) RETURNING *",
            )
            .bind(&req.privy_id)
            .bind(&req.email)
            .bind(&req.name)
            .fetch_one(&mut *tx)
            .await
            .expect("Error creating new user");

            // Create wallet with direct type
            let wallet: Wallet = sqlx::query_as(
                "INSERT INTO wallet (user_id, currency, balance, wallet_type, wallet_address) VALUES ($1, $2, $3, $4, $5) RETURNING *",
            )
            .bind(created_user.id)
            .bind(Currency::MON.to_string())
            .bind(0.0)
            .bind(WalletType::DIRECT.to_string())
            .bind(req.wallet_address.clone().unwrap_or_else(|| "".to_string()))
            .fetch_one(&mut *tx)
            .await
            .expect("Failed to create wallet");

            tx.commit().await.expect("Failed to commit transaction");

            HttpResponse::Created().json(json!({
                "user_id": created_user.id,
                "currency": Currency::MON.to_string() ,
                "balance": 0.0,
                "wallet_type": WalletType::DIRECT.to_string(),
                "wallet_address": wallet.wallet_address.unwrap_or_else(|| "".to_string())
            }))
        }
    }
}

#[actix_web::post("/user-details/{user_id}")]
async fn update_user_details(
    path: web::Path<i32>,
    req: web::Json<UpdateUserDetailsRequest>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    let user_id = path.into_inner();
    let AppState { pool } = &**app_state;

    let mut tx = pool.begin().await.expect("Failed to start transaction");

    let existing_user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .expect("Error fetching user");

    match existing_user {
        Some(user) => {
            sqlx::query("UPDATE users SET name = $1, email = $2 WHERE id = $3")
                .bind(req.name.clone().unwrap_or(user.name))
                .bind(req.email.clone().unwrap_or(user.email))
                .bind(user_id)
                .execute(&mut *tx)
                .await
                .expect("Error updating user details");

            tx.commit().await.expect("Failed to commit transaction");

            HttpResponse::Ok().body("User details updated successfully")
        }
        None => HttpResponse::NotFound().body("User not found"),
    }
}

#[actix_web::get("/user-stats/{user_id}/{network}")]
async fn get_user_stats(
    path: web::Path<(i32, String)>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    let (user_id, network) = path.into_inner();
    let AppState { pool } = &**app_state;

    let stats: UserNetworkPnl =
        sqlx::query_as("SELECT * FROM user_network_pnl WHERE user_id = $1 AND network = $2")
            .bind(user_id)
            .bind(network)
            .fetch_one(pool)
            .await
            .expect("Error fetching user stats");

    HttpResponse::Ok().json(stats)
}

#[actix_web::get("/leaderboard/{network}/{timeframe}")]
async fn get_leaderboard(
    path: web::Path<(String, String)>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    let (network, timeframe) = path.into_inner();
    let AppState { pool } = &**app_state;

    let leaders: Vec<LeaderboardEntry> = match timeframe.as_str() {
        "24h" => db::get_leaderboard_24h(pool, &network, 100)
            .await
            .expect("Failed to fetch leaderboard"),
        "all" => db::get_leaderboard_all_time(pool, &network, 100)
            .await
            .expect("Failed to fetch leaderboard"),
        _ => return HttpResponse::BadRequest().body("Invalid timeframe"),
    };

    HttpResponse::Ok().json(leaders)
}

#[actix_web::get("/health")]
async fn health_check() -> impl Responder {
    info!("Health check request arrived");
    HttpResponse::Ok().content_type("text/plain").body("OK")
}

#[actix_web::post("/deposit")]
async fn deposit(
    deposit_request: web::Json<DepositRequest>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    let AppState { pool } = &**app_state;
    let deposit_request = deposit_request.into_inner();
    info!("Deposit request arrived");
    info!("Deposit request: {:?}", deposit_request);

    let mut tx = pool.begin().await.expect("Failed to start transaction");

    let wallet: Wallet =
        sqlx::query_as("SELECT * FROM wallet WHERE user_id = $1 AND currency = $2")
            .bind(deposit_request.user_id)
            .bind(deposit_request.currency.to_string())
            .fetch_one(&mut *tx)
            .await
            .expect("Error fetching wallet");

    let new_balance = deposit_request.amount + wallet.balance;

    sqlx::query(
        "UPDATE wallet SET balance = $1, updated_at = NOW() WHERE user_id = $2 AND currency = $3",
    )
    .bind(new_balance)
    .bind(deposit_request.user_id)
    .bind(deposit_request.currency.to_string())
    .execute(&mut *tx)
    .await
    .expect("Error updating wallet balance");

    // Record the transaction
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, currency, tx_type, tx_hash) 
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(deposit_request.user_id)
    .bind(deposit_request.amount)
    .bind(deposit_request.currency.to_string())
    .bind(TxType::DEPOSIT.to_string())
    .bind(&deposit_request.tx_hash)
    .execute(&mut *tx)
    .await
    .expect("Error recording transaction");

    tx.commit().await.expect("Failed to commit transaction");

    HttpResponse::Ok().json(json!({
        "user_id": deposit_request.user_id,
        "currency": deposit_request.currency,
        "balance": new_balance,
        "tx_hash": deposit_request.tx_hash
    }))
}

#[actix_web::post("/withdraw")]
async fn withdraw(
    withdraw_req: web::Json<WithdrawRequest>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    let AppState { pool } = &**app_state;
    let withdraw_req = withdraw_req.into_inner();
    info!("Attempting to withdraw");

    let mut tx = pool.begin().await.expect("Failed to start transaction");

    let wallet: Wallet =
        sqlx::query_as("SELECT * FROM wallet WHERE user_id = $1 AND currency = $2")
            .bind(withdraw_req.user_id)
            .bind(withdraw_req.currency.to_string())
            .fetch_one(&mut *tx)
            .await
            .expect("Error fetching wallet");

    if withdraw_req.amount > wallet.balance {
        return HttpResponse::BadRequest().body("Insufficient balance");
    }

    let tx_hash = match transfer_funds(&withdraw_req.withdraw_address, withdraw_req.amount).await {
        Ok(hash) => hash,
        Err(e) => {
            return HttpResponse::InternalServerError().body(format!("Transfer failed: {}", e))
        }
    };

    let new_balance = wallet.balance - withdraw_req.amount;

    // Update the user's wallet balance
    sqlx::query(
        "UPDATE wallet SET balance = $1, updated_at = NOW() WHERE user_id = $2 AND currency = $3",
    )
    .bind(new_balance)
    .bind(withdraw_req.user_id)
    .bind(withdraw_req.currency.to_string())
    .execute(&mut *tx)
    .await
    .expect("Error updating wallet balance");

    // // Generate transaction hash for withdrawal
    // let timestamp = SystemTime::now()
    //     .duration_since(SystemTime::UNIX_EPOCH)
    //     .unwrap()
    //     .as_millis();
    // let tx_hash = format!("withdraw_{}", timestamp);

    // Record the transaction
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, currency, tx_type, tx_hash) 
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(withdraw_req.user_id)
    .bind(withdraw_req.amount)
    .bind(withdraw_req.currency.to_string())
    .bind(TxType::WITHDRAWAL.to_string())
    .bind(&tx_hash)
    .execute(&mut *tx)
    .await
    .expect("Error recording transaction");

    tx.commit().await.expect("Failed to commit transaction");

    HttpResponse::Ok().json(json!({
        "user_id": withdraw_req.user_id,
        "currency": withdraw_req.currency,
        "balance": new_balance,
        "tx_hash": tx_hash,
        "withdraw_address": withdraw_req.withdraw_address
    }))
}

struct AppState {
    pool: Pool<Postgres>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting the wallet service");
    let pool = establish_connection().await;
    let app_state = web::Data::new(AppState { pool });

    info!("Starting HTTP server on 0.0.0.0:8080");
    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .wrap(Logger::default())
            .wrap(Cors::permissive())
            .service(health_check)
            .service(deposit)
            .service(withdraw)
            .service(fetch_or_create_user)
            .service(get_user_stats)
            .service(get_leaderboard)
            .service(update_user_details)
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}

// async fn start_account_watchers(pool: sqlx::Pool<sqlx::Sqlite>, tx: mpsc::Sender<Pubkey>) {
//     let mut conn = pool.acquire().await.expect("DB Connection failed");

//     loop
//     let users: Vec<User> = sqlx::query_as("SELECT user_pda FROM users")
//         .fetch_all(&mut conn)
//         .await
//         .expect("Failed to fetch users");

//     for user in users {
//         let account_pubkey = Pubkey::from_str(&user.user_pda).expect("Invalid pubkey");
//         tx.send(account_pubkey)
//             .await
//             .expect("Failed to send account to channel");
//     }
// }
