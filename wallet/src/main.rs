use std::str::FromStr;

use actix_cors::Cors;
use actix_web::{middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use common::{
    db,
    models::{self, Pnl},
    utils::{self, DepositRequest, UserDetailsRequest, WithdrawRequest},
};
use db::establish_connection;
use deposits::DepositService;
use dotenv::dotenv;
use models::{User, Wallet};

use serde_json::json;
use solana_sdk::{nonce::state::Data, pubkey::Pubkey};
use sqlx::{Pool, Sqlite};
use utils::TxType;

const SOL_TO_LAMPORTS: u64 = 1_000_000_000;
#[actix_web::post("/user-details")]
async fn fetch_or_create_user(
    req: web::Json<UserDetailsRequest>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    let AppState {
        pool,
        deposit_service,
    } = &**app_state;

    println!("Got a request");
    let mut conn = pool
        .acquire()
        .await
        .expect("failed to get connection from the pool");

    // Check if the user already exists
    let existing_user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE email = ?")
        .bind(&req.email)
        .fetch_optional(&mut conn)
        .await
        .expect("Error fetching user");

    match existing_user {
        Some(user) => HttpResponse::Created().json(json!({
            "id": user.id,
            "currency": "SOL",
            "balance": 0,
            "user_pda": user.user_pda

        })),
        None => {
            // create user program derived address and add a listener to its account changes
            // TODO: check if user pda already exists and creat a new
            let user_pda = deposit_service
                .generate_deposit_address()
                .expect("Failed to create deposit address");

            // User does not exist, create a new user
            sqlx::query("INSERT INTO users (clerk_id, email, name, user_pda) VALUES (?, ?, ?, ?)")
                .bind(&req.clerk_id)
                .bind(&req.email)
                .bind(&req.name)
                .bind(user_pda.to_string())
                .execute(&mut conn)
                .await
                .expect("Error creating new user");

            // Fetch the newly created user to return their details
            let created_user: User = sqlx::query_as("SELECT * FROM users WHERE email = ?")
                .bind(&req.email)
                .fetch_one(&mut conn)
                .await
                .expect("Error fetching newly created user");

            db::create_user_and_update_tables(&pool, &created_user)
                .await
                .expect("Failed to update tables");

            HttpResponse::Created().json(json!({
                "user_id": created_user.id,
                "currency": "SOL",
                "balance": 0,
                "user_pda": user_pda.to_string()
            }))
        }
    }
}

#[actix_web::get("/pnl/{user_id}")]
async fn get_pnl(user_id: web::Path<String>, app_state: web::Data<AppState>) -> impl Responder {
    let user_id: u32 = user_id.into_inner().parse().unwrap();
    let AppState {
        pool,
        deposit_service: _,
    } = &**app_state;

    let mut conn = pool.acquire().await.unwrap();
    let user_pnl: Pnl = sqlx::query_as("SELECT * FROM pnl where user_id = ?")
        .bind(user_id)
        .fetch_one(&mut conn)
        .await
        .expect("Error fetching wallet");

    HttpResponse::Ok().json(user_pnl)
}

#[actix_web::post("/deposit")]
async fn deposit(
    deposit_request: web::Json<DepositRequest>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    let AppState {
        pool,
        deposit_service: _,
    } = &**app_state;
    println!("Deposit request arrived");

    let mut conn = pool
        .acquire()
        .await
        .expect("Failed to get a connection from the pool");

    let wallet: Wallet = sqlx::query_as("SELECT * FROM wallet where user_id = ? and currency = ?")
        .bind(deposit_request.user_id)
        .bind(deposit_request.currency.to_string())
        .fetch_one(&mut conn)
        .await
        .expect("Error fetching wallet");

    println!("Wallet: {:?}", wallet);

    let new_balance = deposit_request.amount + wallet.balance;

    sqlx::query("update wallet set balance = ? where user_id = ? and currency = ?")
        .bind(new_balance)
        .bind(deposit_request.user_id)
        .bind(deposit_request.currency.to_string())
        .execute(&mut conn)
        .await
        .expect("Error updating wallet balance");

    // // Record the transaction
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, currency, tx_type, tx_hash) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(deposit_request.user_id)
    .bind(deposit_request.amount)
    .bind(deposit_request.currency.to_string())
    .bind(deposit_request.tx_type.to_string())
    .bind(deposit_request.tx_hash.clone())
    .execute(&mut conn)
       .await
    .expect("Error recording transaction");

    HttpResponse::Ok().json(json!({
        "user_id": deposit_request.user_id,
        "currency": deposit_request.currency,
        "balance": new_balance
    }))
}

#[actix_web::post("/withdraw")]
async fn withdraw(
    withdraw_req: web::Json<WithdrawRequest>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    println!("Attempting to withdraw");
    let AppState {
        pool,
        deposit_service,
    } = &**app_state;
    println!("Received withdraw request");
    let mut conn = pool
        .acquire()
        .await
        .expect("Failed to get a connection from the pool");

    // Fetch the user's current wallet balance
    let current_balance: (f64,) =
        sqlx::query_as("SELECT balance FROM wallet WHERE user_id = ? and currency = ?")
            .bind(withdraw_req.user_id)
            .bind(withdraw_req.currency.to_string())
            .fetch_one(&mut conn)
            .await
            .expect("Error loading user");

    if withdraw_req.amount > current_balance.0 {
        return HttpResponse::BadRequest().body("Insufficient balance");
    }

    //FIXME: transfer the req.amount to user
    let withdraw_txhash = deposit_service
        .withdraw_to_user_from_treasury(
            withdraw_req.withdraw_address.clone(),
            (withdraw_req.amount * SOL_TO_LAMPORTS as f64) as u64,
        )
        .await
        .unwrap();

    println!("Withdrawn tx hash: {:?}", withdraw_txhash);

    // Deduct the amount from the user's wallet
    let new_balance = current_balance.0 - withdraw_req.amount;

    // Update the user's wallet balance
    sqlx::query("UPDATE wallet SET balance = ? WHERE user_id = ? and currency = ?")
        .bind(new_balance)
        .bind(withdraw_req.user_id)
        .bind(withdraw_req.currency.to_string())
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");

    // Record the transaction
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, currency, tx_type, tx_hash) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(withdraw_req.user_id)
    .bind(withdraw_req.amount)
    .bind(withdraw_req.currency.to_string())
    .bind(TxType::WITHDRAWAL.to_string())
    .bind(withdraw_txhash.clone())
    .execute(&mut conn)
    .await
    .expect("Error recording transaction");

    println!(
        "Withdrawal of {} successful. New balance: {}",
        withdraw_req.amount, new_balance
    );
    return HttpResponse::Ok().json(json!(
        {
            "user_id": withdraw_req.user_id,
            "currency": withdraw_req.currency,
            "balance": new_balance,
            "tx_hash": withdraw_txhash.clone(),
        }
    ));
}

struct AppState {
    pool: Pool<Sqlite>,
    deposit_service: DepositService,
}
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load environment variables from .env file
    dotenv().ok();
    println!("Starting the wallet");

    let pool = establish_connection().await;

    let program_id = Pubkey::from_str("FFT8CyM7DnNoWG2AukQqCEyNtZRLJvxN9WK6S7mC5kLP").unwrap();

    let cwd = std::env::current_dir().unwrap();
    let deposit_service = DepositService::new(cwd.join("treasury-keypair.json"), program_id);

    let app_state = web::Data::new(AppState {
        pool,
        deposit_service,
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .wrap(Logger::default())
            .wrap(Cors::permissive())
            .service(deposit)
            .service(withdraw)
            .service(fetch_or_create_user)
    })
    .bind("127.0.0.1:8080")?
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
