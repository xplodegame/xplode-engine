use common::agg_mod;
use dotenv::dotenv;
use game::GameServer;
use prometheus::{Encoder, TextEncoder};
use tracing::info;
use warp::Filter;

agg_mod!(board game player seed_gen discovery metrics);

async fn metrics_handler() -> Result<impl warp::Reply, warp::Rejection> {
    info!("Metrics handler called");
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();
    Ok(warp::http::Response::builder()
        .header("Content-Type", encoder.format_type())
        .body(buffer))
}

async fn health_handler() -> Result<impl warp::Reply, warp::Rejection> {
    Ok(warp::http::Response::builder().body("OK"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables from .env file if it exists
    dotenv().ok();
    // Set the default log level to info
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO) // Set the log level to INFO
        .init();
    info!("Starting the game server");

    // Create health endpoint
    let health_route = warp::path!("health")
        .and(warp::get())
        .and_then(health_handler);

    // Create metrics endpoint
    let metrics_route = warp::path!("metrics")
        .and(warp::get())
        .and_then(metrics_handler);

    // Start metrics server on port 9092
    let metrics_addr = ([0, 0, 0, 0], 9092);
    tokio::spawn(warp::serve(metrics_route.or(health_route)).run(metrics_addr));
    info!("Metrics server listening on :9092");

    // Start the game server
    let game_server = GameServer::new().await;
    game_server.start("0.0.0.0:3000").await?;
    Ok(())
}
