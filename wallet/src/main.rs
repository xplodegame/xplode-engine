use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use common::{db, models, utils};
use db::establish_connection;
use dotenv::dotenv;
use models::{User, Wallet};

use serde::Deserialize;
use serde_json::json;
use utils::{Currency, TxType};

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
    tx_hash: String,
    tx_type: TxType,
}

#[actix_web::post("/user-details")]
async fn fetch_or_create_user(
    req: web::Json<UserDetailsRequest>,
    pool: web::Data<sqlx::Pool<sqlx::Sqlite>>,
) -> impl Responder {
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
            HttpResponse::Ok().json(user)
        }
        None => {
            // User does not exist, create a new user
            sqlx::query("INSERT INTO users (clerk_id, email, name) VALUES (?, ?, ?)")
                .bind(&req.clerk_id)
                .bind(&req.email)
                .bind(&req.name)
                .execute(&mut conn)
                .await
                .expect("Error creating new user");

            // Fetch the newly created user to return their details
            let created_user: User = sqlx::query_as("SELECT * FROM users WHERE email = ?")
                .bind(&req.email)
                .fetch_one(&mut conn)
                .await
                .expect("Error fetching newly created user");

            HttpResponse::Created().json(created_user) // Return the created user details
        }
    }
}

#[actix_web::post("/deposit")]
async fn deposit(
    req: web::Json<DepositRequest>,
    pool: web::Data<sqlx::Pool<sqlx::Sqlite>>,
) -> impl Responder {
    println!("Deposit request arrived");

    let mut conn = pool
        .acquire()
        .await
        .expect("Failed to get a connection from the pool");

    let wallet: Option<Wallet> = sqlx::query_as("SELECT * FROM wallet where user_id = ?")
        .bind(&req.user_id)
        .fetch_optional(&mut conn)
        .await
        .expect("Error fetching wallet");

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

    // Record the transaction
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
    pool: web::Data<sqlx::Pool<sqlx::Sqlite>>,
) -> impl Responder {
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
    .bind(req.tx_type.to_string())
    .bind(req.tx_hash.clone())
    .execute(&mut conn)
    .await
    .expect("Error recording transaction");

    println!(
        "Withdrawal of {} successful. New balance: {}",
        req.amount, new_balance
    );
    HttpResponse::Ok().json(json!(
        {
            "user_id": req.user_id,
            "currency": req.currency,
            "balance": new_balance
        }
    ))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load environment variables from .env file
    dotenv().ok();

    let pool = establish_connection().await;

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(Cors::permissive())
            .service(deposit)
            .service(withdraw)
            .service(fetch_or_create_user)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
