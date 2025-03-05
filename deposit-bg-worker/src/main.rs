use std::{env, str::FromStr, time::Duration};

use common::{db::establish_connection, models::User};
use deposits::DepositService;
use solana_sdk::pubkey::Pubkey;
use tokio::time::sleep;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // let env = env::var("APP_ENV").unwrap_or_else(|_| "dev".to_string());
    // let env_file = format!(".env.{}", env);
    // dotenv::from_filename(env_file)
    //     .ok()
    //     .expect("Failed to load .env file");
    tracing_subscriber::fmt::init();
    info!("Starting the deposit background service");
    let program_id = Pubkey::from_str("FFT8CyM7DnNoWG2AukQqCEyNtZRLJvxN9WK6S7mC5kLP").unwrap();

    let cwd = std::env::current_dir().unwrap();
    let service = DepositService::new(cwd.join("treasury-keypair.json"), program_id);

    let pool = establish_connection();
    let mut conn = pool.await.acquire().await.expect("DB conn failed");

    loop {
        info!("Hello");
        let users: Vec<User> = sqlx::query_as("SELECT * FROM users")
            .fetch_all(&mut conn)
            .await
            .expect("Fqailed to fetch users");

        let users_pubkeys: Vec<_> = users
            .iter()
            .map(|user| Pubkey::from_str(&user.user_pda).unwrap())
            .collect();

        service.check_deposits(users_pubkeys).await.unwrap();

        sleep(Duration::from_secs(10)).await;
    }
}
