use common::agg_mod;
//use clap::{Parser, Subcommand};
// use game::GameManager;
use dotenv::dotenv;
use game::GameServer;
use std::net::SocketAddr;
use tokio::task;
use tracing::info;
use warp::Filter;

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

    // // Start a simple HTTP server for health checks
    // let health_route = warp::path("health").map(|| "OK");
    // let health_addr: SocketAddr = "0.0.0.0:3001".parse()?;
    // let health_server = task::spawn(warp::serve(health_route).run(health_addr));

    // Start the game server
    let game_server = GameServer::new().await;
    game_server.start("0.0.0.0:3000").await?;
    // let game_server_task = task::spawn(async move {
    //     if let Err(e) = game_server.start("0.0.0.0:3000").await {
    //         tracing::error!("Game server error: {}", e);
    //     }
    // });

    // // Keep the program running by waiting for both tasks
    // tokio::select! {
    //     _ = health_server => tracing::error!("Health server stopped"),
    //     _ = game_server_task => tracing::error!("Game server stopped"),
    // }

    Ok(())
}
