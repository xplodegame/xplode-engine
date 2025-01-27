use std::{str::FromStr, time::Duration};

use common::{db::establish_connection, models::User};
use deposits::DepositService;
use dotenv::dotenv;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let current_dir = std::env::current_dir()?;
    let service = DepositService::new(current_dir.join("treasury-keypair.json"));
    let pool = establish_connection();
    let mut conn = pool.await.acquire().await.expect("DB conn failed");

    loop {
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
