use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use db::establish_connection;
use dotenv::dotenv;
use razorpay_client::{RazorpayClient, VerifyPaymentRequest};
use serde::Deserialize;
use std::env;

mod db;
mod razorpay_client;

#[derive(Deserialize)]
struct DepositRequest {
    user_id: Option<u32>,
    amount: u32,
    currency: String,
}

#[derive(Deserialize)]
struct WithdrawRequest {
    user_id: Option<u32>,
    amount: u32,
}

#[actix_web::post("/deposit")]
async fn deposit(
    req: web::Json<DepositRequest>,
    client: web::Data<RazorpayClient>,
    pool: web::Data<sqlx::Pool<sqlx::Sqlite>>,
) -> impl Responder {
    println!("Request arrived");

    let user_id = req.user_id.unwrap_or_else(|| 1);
    let mut conn = pool
        .acquire()
        .await
        .expect("Failed to get a connection from the pool");
    match client
        .create_order(req.user_id, req.amount, &req.currency)
        .await
    {
        Ok(order) => {
            // Fetch the user's current wallet balance
            let current_balance: (u32,) =
                sqlx::query_as("SELECT wallet_amount FROM users WHERE id = ?")
                    .bind(user_id)
                    .fetch_one(&mut conn)
                    .await
                    .expect("Error loading user");

            let new_balance = current_balance.0 + req.amount;

            // Update the user's wallet balance
            sqlx::query("UPDATE users SET wallet_amount = ? WHERE id = ?")
                .bind(new_balance)
                .bind(user_id)
                .execute(&mut conn)
                .await
                .expect("Error updating user wallet");

            sqlx::query(
                "INSERT INTO transactions (user_id, amount, transaction_type) VALUES (?, ?, ?)",
            )
            .bind(user_id)
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

    let user_id = req.user_id.unwrap_or_else(|| 1);

    // Fetch the user's current wallet balance
    let current_balance: (u32,) = sqlx::query_as("SELECT wallet_amount FROM users WHERE id = ?")
        .bind(user_id)
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
        .bind(user_id)
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");

    // Record the transaction
    sqlx::query("INSERT INTO transactions (user_id, amount, transaction_type) VALUES (?, ?, ?)")
        .bind(user_id)
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
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
