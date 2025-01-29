use std::str::FromStr;

use actix_cors::Cors;
use actix_web::{middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use common::{db, models, utils};
use db::establish_connection;
use deposits::DepositService;
use dotenv::dotenv;
use models::{User, Wallet};

use serde::Deserialize;
use serde_json::json;
use solana_sdk::pubkey::Pubkey;
use sqlx::{Pool, Sqlite};
use utils::{Currency, TxType};

const SOL_TO_LAMPORTS: u64 = 1_000_000_000;

#[derive(Deserialize)]
struct UserDetailsRequest {
    name: String,
    email: String,
    clerk_id: String,
}

#[derive(Deserialize)]
struct DepositRequest {
    user_id: u32,
    amount: f64,
    currency: Currency,
    tx_type: TxType,
    tx_hash: String,
}

#[derive(Deserialize)]
struct WithdrawRequest {
    user_id: u32,
    amount: f64,
    currency: Currency,
    withdraw_address: String,
}

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
        Some(user) => {
            // User exists, return their details
            let wallet: Option<Wallet> =
                sqlx::query_as("SELECT * FROM wallet where user_id = ? and currency = ?")
                    .bind(user.id)
                    .bind("SOL".to_string())
                    .fetch_optional(&mut conn)
                    .await
                    .expect("Error fetching wallet");

            if wallet.is_some() {
                HttpResponse::Created().json(json!({
                    "id": user.id,
                    "currency": "SOL",
                    "balance": wallet.unwrap().balance,
                    "user_pda": user.user_pda

                })) // Return the created user details
            } else {
                HttpResponse::Created().json(json!({
                    "id": user.id,
                    "currency": "SOL",
                    "balance": 0,
                    "user_pda": user.user_pda

                }))
            }
        }
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

            let wallet: Option<Wallet> =
                sqlx::query_as("SELECT * FROM wallet where user_id = ? and currency = ?")
                    .bind(created_user.id)
                    .bind("SOL".to_string())
                    .fetch_optional(&mut conn)
                    .await
                    .expect("Error fetching wallet");

            if wallet.is_some() {
                HttpResponse::Created().json(json!({
                    "user_id": created_user.id,
                    "currency": "SOL",
                    "balance": wallet.unwrap().balance,
                    "user_pda": user_pda.to_string()

                })) // Return the created user details
            } else {
                HttpResponse::Created().json(json!({
                    "user_id": created_user.id,
                    "currency": "SOL",
                    "balance": 0,
                    "user_pda": user_pda.to_string()
                }))
            }
        }
    }
}

#[actix_web::post("/deposit")]
async fn deposit(req: web::Json<DepositRequest>, app_state: web::Data<AppState>) -> impl Responder {
    let AppState {
        pool,
        deposit_service: _,
    } = &**app_state;
    println!("Deposit request arrived");

    let mut conn = pool
        .acquire()
        .await
        .expect("Failed to get a connection from the pool");

    let wallet: Option<Wallet> =
        sqlx::query_as("SELECT * FROM wallet where user_id = ? and currency = ?")
            .bind(req.user_id)
            .bind(req.currency.to_string())
            .fetch_optional(&mut conn)
            .await
            .expect("Error fetching wallet");

    println!("Wallet: {:?}", wallet);

    let mut new_balance = req.amount;
    if let Some(wallet) = wallet {
        new_balance = new_balance + wallet.balance;
        sqlx::query("update wallet set balance = ? where user_id = ? and currency = ?")
            .bind(new_balance)
            .bind(req.user_id)
            .bind(req.currency.to_string())
            .execute(&mut conn)
            .await
            .expect("Error updating wallet balance");
    } else {
        sqlx::query("INSERT INTO wallet (user_id, currency, balance) VALUES (?, ?, ?)")
            .bind(req.user_id) // Bind the user_id
            .bind(req.currency.to_string()) // Set a default currency, e.g., USD
            .bind(req.amount) // Initialize balance to deposit amount
            .execute(&mut conn)
            .await
            .expect("Error creating initial wallet");
    }

    // // Record the transaction
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, currency, tx_type, tx_hash) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(req.user_id)
    .bind(req.amount)
    .bind(req.currency.to_string())
    .bind(req.tx_type.to_string())
    .bind(req.tx_hash.clone())
    .execute(&mut conn)
       .await
    .expect("Error recording transaction");

    HttpResponse::Ok().json(json!({
        "user_id": req.user_id,
        "currency": req.currency,
        "balance": new_balance
    }))
}

#[actix_web::post("/withdraw")]
async fn withdraw(
    req: web::Json<WithdrawRequest>,
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
            .bind(req.user_id)
            .bind(req.currency.to_string())
            .fetch_one(&mut conn)
            .await
            .expect("Error loading user");

    if req.amount > current_balance.0 {
        return HttpResponse::BadRequest().body("Insufficient balance");
    }

    //FIXME: transfer the req.amount to user
    let withdraw_txhash = deposit_service
        .withdraw_to_user_from_treasury(
            req.withdraw_address.clone(),
            (req.amount * SOL_TO_LAMPORTS as f64) as u64,
        )
        .await
        .unwrap();

    println!("Withdrawn tx hash: {:?}", withdraw_txhash);

    // Deduct the amount from the user's wallet
    let new_balance = current_balance.0 - req.amount;

    // Update the user's wallet balance
    sqlx::query("UPDATE wallet SET balance = ? WHERE user_id = ? and currency = ?")
        .bind(new_balance)
        .bind(req.user_id)
        .bind(req.currency.to_string())
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");

    // Record the transaction
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, currency, tx_type, tx_hash) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(req.user_id)
    .bind(req.amount)
    .bind(req.currency.to_string())
    .bind(TxType::WITHDRAWAL.to_string())
    .bind(withdraw_txhash.clone())
    .execute(&mut conn)
    .await
    .expect("Error recording transaction");

    println!(
        "Withdrawal of {} successful. New balance: {}",
        req.amount, new_balance
    );
    return HttpResponse::Ok().json(json!(
        {
            "user_id": req.user_id,
            "currency": req.currency,
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
