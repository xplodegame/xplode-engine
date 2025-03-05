use std::env;

use common::agg_mod;
//use clap::{Parser, Subcommand};
// use game::GameManager;
use dotenv::dotenv;
use game::GameServer;
use tracing::info;

//mod macros;

agg_mod!(board game player seed_gen);
//
//#[derive(Parser)]
//#[clap(author, version, about, long_about = None)]
//struct Cli {
//    #[clap(subcommand)]
//    command: Commands,
//}

//#[derive(Subcommand)]
//enum Commands {
//    // Server
//    Server,
//
//    // Client
//    Client,
//}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables from .env file if it exists
    dotenv().ok();
    // Set the default log level to info
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO) // Set the log level to INFO
        .init();
    info!("Starting the game server");

    // // Environment variables are already set in docker-compose.yml
    // // We can still try to load from .env files as fallback for local development
    // let env = env::var("APP_ENV").unwrap_or_else(|_| "dev".to_string());
    // if env == "dev" || env == "local" {
    //     let env_file = format!(".env.{}", env);
    //     dotenv::from_filename(env_file).ok();
    // }

    // info!("Starting the deposit background service");

    let game_server = GameServer::new().await;
    game_server.start("0.0.0.0:3000").await?;

    Ok(())
}
