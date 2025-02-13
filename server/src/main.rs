use common::agg_mod;
//use clap::{Parser, Subcommand};
// use game::GameManager;
use dotenv::dotenv;
use game::GameServer;

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
    dotenv().ok();
    //println!("Starting the server");
    //let cli = Cli::parse();
    //
    //match cli.command {
    //    Commands::Server => {
    //        let game_server = GameServer::new();
    //
    //        game_server.start("127.0.0.1:3000").await?;
    //    }
    //    // Commands::Client => {
    //    //     let client = GameClient::new("ws://127.0.0.1:3000").await?;
    //    //     client.run_client().await?;
    //    // }
    //    _ => {}
    //}
    //

    let game_server = GameServer::new().await;
    game_server.start("127.0.0.1:3000").await?;

    Ok(())
}
