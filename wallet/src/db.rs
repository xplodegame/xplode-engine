use std::env;

use anyhow::Ok;
use dotenv::dotenv;
use sqlx::{Pool, SqlitePool};

use crate::models::User;

pub async fn establish_connection() -> Pool<sqlx::Sqlite> {
    dotenv().ok();

    let db_url = env::var("DATABASE_URL").unwrap();
    println!("Db url: {:?} ", db_url);

    SqlitePool::connect(&db_url)
        .await
        .expect("Failed to create pool")
}

pub async fn get_user(pool: &Pool<sqlx::Sqlite>, user_id: i32) -> anyhow::Result<User> {
    let mut conn = pool
        .acquire()
        .await
        .expect("failed to get connection from the pool");

    let user: User = sqlx::query_as("Select * from users where id = ?")
        .bind(user_id)
        .fetch_one(&mut conn)
        .await
        .expect("Failed to fetch user");
    Ok(user)
}

pub async fn update_user(
    pool: &Pool<sqlx::Sqlite>,
    user_id: i32,
    new_balance: i32,
) -> anyhow::Result<()> {
    println!("Updating user");
    let mut conn = pool
        .acquire()
        .await
        .expect("failed to get connection from the pool");

    // Update the user's wallet balance
    sqlx::query("UPDATE users SET wallet_amount = ? WHERE id = ?")
        .bind(new_balance)
        .bind(user_id)
        .execute(&mut conn)
        .await
        .expect("Error updating user wallet");

    Ok(())
}
