use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use db::establish_connection;
use dotenv::dotenv;
use models::User;

use razorpay::razorpay_client::{RazorpayClient, VerifyPaymentRequest};
use serde::Deserialize;
use std::env;

pub mod db;
mod models;
mod razorpay;

#[derive(Deserialize)]
struct UserDetailsRequest {
    name: String,
    email: String,
    clerk_id: String,
}

#[derive(Deserialize)]
struct DepositRequest {
    user_id: u32,
    amount: u32,
    currency: String,
}

#[derive(Deserialize)]
struct WithdrawRequest {
    user_id: u32,
    amount: u32,
}

#[actix_web::post("/user-details")]
async fn user_details(
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
            let new_user = sqlx::query(
                "INSERT INTO users (clerk_id, email, name, wallet_amount) VALUES (?, ?, ?, ?)",
            )
            .bind(&req.clerk_id)
            .bind(&req.email)
            .bind(&req.name)
            .bind(0) // Assuming new users start with a wallet amount of 0
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
    client: web::Data<RazorpayClient>,
    pool: web::Data<sqlx::Pool<sqlx::Sqlite>>,
) -> impl Responder {
    println!("Deposit request arrived");

    let mut conn = pool
        .acquire()
        .await
        .expect("Failed to get a connection from the pool");

    let user: User = sqlx::query_as("SELECT * FROM users WHERE id = ?")
        .bind(&req.user_id)
        .fetch_one(&mut conn)
        .await
        .expect("Error fetching user");

    match client.create_order(req.amount, &req.currency, &user).await {
        Ok(order) => {
            // Fetch the user's current wallet balance
            let current_balance = user.wallet_amount;

            let new_balance = current_balance + (req.amount as i32);

            // Update the user's wallet balance
            sqlx::query("UPDATE users SET wallet_amount = ? WHERE id = ?")
                .bind(new_balance)
                .bind(req.user_id)
                .execute(&mut conn)
                .await
                .expect("Error updating user wallet");

            sqlx::query(
                "INSERT INTO transactions (user_id, amount, transaction_type) VALUES (?, ?, ?)",
            )
            .bind(req.user_id)
            .bind(req.amount)
            .bind("deposit")
            .execute(&mut conn)
            .await
            .expect("Error recording transaction");

            HttpResponse::Ok().json(order)
        }
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

#[actix_web::post("/verify-payment")]
async fn verify_payment(
    req: web::Json<VerifyPaymentRequest>,
    client: web::Data<RazorpayClient>,
) -> impl Responder {
    println!("Verification payment request arrived");
    match client
        .verify_payment(
            &req.razorpay_payment_id,
            &req.razorpay_order_id,
            &req.razorpay_signature,
        )
        .await
    {
        Ok(true) => {
            println!("Verification succcess");
            HttpResponse::Ok().finish()
        }
        Ok(false) => HttpResponse::Unauthorized().finish(),
        Err(err) => {
            println!("error encounter: {:?}", err);
            HttpResponse::InternalServerError().finish()
        }
    }
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
    let current_balance: (u32,) = sqlx::query_as("SELECT wallet_amount FROM users WHERE id = ?")
        .bind(req.user_id)
        .fetch_one(&mut conn)
        .await
        .expect("Error loading user");

    if req.amount > current_balance.0 {
        return HttpResponse::BadRequest().body("Insufficient balance");
    }

    // Deduct the amount from the user's wallet
    let new_balance = current_balance.0 - req.amount;

    // Update the user's wallet balance
    sqlx::query("UPDATE users SET wallet_amount = ? WHERE id = ?")
        .bind(new_balance)
        .bind(req.user_id)
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");

    // Record the transaction
    sqlx::query("INSERT INTO transactions (user_id, amount, transaction_type) VALUES (?, ?, ?)")
        .bind(req.user_id)
        .bind(req.amount)
        .bind("withdrawal")
        .execute(&mut conn)
        .await
        .expect("Error recording transaction");

    println!(
        "Withdrawal of {} successful. New balance: {}",
        req.amount, new_balance
    );
    HttpResponse::Ok().body(format!(
        "Withdrawal of {} successful. New balance: {}",
        req.amount, new_balance
    ))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load environment variables from .env file
    dotenv().ok();

    let api_key = env::var("RAZORPAY_KEY_ID").unwrap();
    let api_secret = env::var("RAZORPAY_KEY_SECRET").unwrap();
    let razorpay_client = RazorpayClient::new(api_key, api_secret);

    let pool = establish_connection().await;

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(razorpay_client.clone()))
            .app_data(web::Data::new(pool.clone()))
            .wrap(Cors::permissive())
            .service(deposit)
            .service(withdraw)
            .service(verify_payment)
            .service(user_details)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
