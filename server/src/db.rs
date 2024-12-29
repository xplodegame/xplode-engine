// use sqlx::{migrate::MigrateDatabase, Sqlite};

// pub async fn create_database(url: String) -> anyhow::Result<()> {
//     // Create database with connection pooling
//     let database_url = "sqlite://users.db";

//     // Create database if not exists
//     if !Sqlite::database_exists(database_url).await? {
//         Sqlite::create_database(database_url).await?;
//     }

//     sqlx::query("CREATE TABLE IF NOT EXISTS ")

//     Ok(())
// }
