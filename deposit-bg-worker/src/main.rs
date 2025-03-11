use std::{str::FromStr, time::Duration};

use common::{db::establish_connection, models::User};
use deposits::sol::DepositService;
use dotenv::dotenv;
use solana_sdk::pubkey::Pubkey;
use tokio::time::sleep;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    info!("Starting the deposit background service");
    let program_id = std::env::var("PROGRAM_ID").unwrap();

    let cwd = std::env::current_dir().unwrap();
    let service = DepositService::new(cwd.join("treasury-keypair.json"), program_id.to_string());

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
            .filter_map(|user| {
                user.user_pda
                    .as_ref()
                    .map(|pda| Pubkey::from_str(pda).unwrap())
            })
            .collect();

        service.check_deposits(users_pubkeys).await.unwrap();

        sleep(Duration::from_secs(10)).await;
    }
}
