use clap::{Parser, Subcommand};
// use game::GameManager;
use game_ws::{GameClient, GameServer};

mod board;
mod db;
mod game;
mod game_ws;
mod player;
mod seed_gen;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    // Server
    Server,

    // Client
    Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Server => {
            let game_server = GameServer::new();

            game_server.start("127.0.0.1:3000").await?;
        }
        Commands::Client => {
            let client = GameClient::new("ws://127.0.0.1:3000").await?;
            client.run_client().await?;
        }
    }

    Ok(())
}

// fn main() {
//     tokio::runtime::Builder::new_multi_thread()
//         .max_blocking_threads(num_cpus::get())
//         .enable_all()

//         .build()
//         .unwrap()
//         .block_on(amain())
//         .unwrap()
// }

// #[tokio::main]
// async fn main() -> Result<()> {
//     let cli = Cli::parse();

//     let game_manager = GameManager::new()?;

//     match cli.command {
//         Commands::Create => {
//             let game_id = game_manager.create_game().await?;
//             println!("Game created successfully!");
//             println!("Game id: {}", game_id);
//             game_manager.start_game(game_id).await?;
//         }
//         Commands::Join { game_id } => {
//             println!("Join game: {}", game_id);
//             game_manager.join_game(game_id.clone()).await?;

//             println!("Joined game successfully!");
//             game_manager.start_game(game_id).await?;
//         }
//         Commands::Start => {}
//     }

//     Ok(())
// }
