use std::env;

use dotenv::dotenv;
use sqlx::{Pool, Sqlite, SqlitePool};

pub async fn establish_connection() -> Pool<sqlx::Sqlite> {
    dotenv().ok();

    let db_url = env::var("DATABASE_URL").unwrap();

    SqlitePool::connect(&db_url)
        .await
        .expect("Failed to create pool")
}
