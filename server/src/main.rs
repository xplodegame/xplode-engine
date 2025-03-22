use common::agg_mod;
use dotenv::dotenv;
use game::GameServer;
use tracing::info;

agg_mod!(board game player seed_gen discovery);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables from .env file if it exists
    dotenv().ok();
    // Set the default log level to info
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO) // Set the log level to INFO
        .init();
    info!("Starting the game server");

    // Start the game server
    let game_server = GameServer::new().await;
    game_server.start("0.0.0.0:3000").await?;
    Ok(())
}
