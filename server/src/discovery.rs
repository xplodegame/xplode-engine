use anyhow::Result;
use redis::{AsyncCommands, Client};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Instant};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSession {
    pub game_id: String,
    pub server_id: String, // This will be machine_id if available, otherwise UUID
    pub single_bet_size: f64,
    pub min_players: u32,
    pub current_players: u32,
    pub grid_size: u32,
}

#[derive(Clone)]
pub struct DiscoveryService {
    redis: Arc<Client>,
}

impl DiscoveryService {
    pub fn new(redis: Client) -> Self {
        Self {
            redis: Arc::new(redis),
        }
    }

    // Register a new game session
    pub async fn register_game_session(&self, session: GameSession) -> Result<()> {
        let start = Instant::now();
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        let conn_time = start.elapsed();

        // Clone values needed for logging
        let game_id = session.game_id.clone();

        // Store game session info
        let key = format!("game_session:{}", session.game_id);
        let mut pipe = redis::pipe();
        pipe.atomic();
        pipe.hset_multiple(
            &key,
            &[
                ("server_id", session.server_id.clone()),
                ("single_bet_size", session.single_bet_size.to_string()),
                ("min_players", session.min_players.to_string()),
                ("current_players", session.current_players.to_string()),
                ("grid_size", session.grid_size.to_string()),
            ],
        );

        // Add to matchmaking set
        let matchmaking_key = format!(
            "matchmaking:{}:{}:{}",
            session.single_bet_size, session.min_players, session.grid_size
        );
        pipe.sadd(matchmaking_key.clone(), session.game_id);

        // Set TTL for cleanup
        pipe.expire(&key, 120);

        // Execute all commands in a single round trip
        let pipeline_start = Instant::now();
        let _: () = pipe.query_async(&mut conn).await?;
        let pipeline_time = pipeline_start.elapsed();
        let total_time = start.elapsed();

        info!(
            game_id = %game_id,
            conn_latency_ms = %conn_time.as_millis(),
            pipeline_latency_ms = %pipeline_time.as_millis(),
            total_latency_ms = %total_time.as_millis(),
            "Registered game session"
        );

        Ok(())
    }

    pub async fn find_game_session_by_id(&self, game_id: &str) -> Result<Option<GameSession>> {
        info!("Finding game session by id: {}", game_id);
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        let key = format!("game_session:{}", game_id);
        let values: Option<Vec<String>> = conn
            .hget(
                &key,
                &[
                    "server_id",
                    "single_bet_size",
                    "min_players",
                    "current_players",
                    "grid_size",
                ],
            )
            .await?;

        info!("Here 1");
        // Return None if values is None or doesn't have exactly 5 elements
        let values = match values {
            Some(v) if v.len() == 5 => v,
            _ => return Ok(None),
        };

        // Parse values and create session
        let session = GameSession {
            game_id: game_id.to_string(),
            server_id: values[0].clone(),
            single_bet_size: values[1].parse()?,
            min_players: values[2].parse()?,
            current_players: values[3].parse()?,
            grid_size: values[4].parse()?,
        };

        info!("Here 2");
        // Only return the session if it has room for more players
        Ok(if session.current_players < session.min_players {
            Some(session)
        } else {
            None
        })
    }

    // Find best matching game session based on bet size and player count
    pub async fn find_game_session(
        &self,
        single_bet_size: f64,
        min_players: u32,
        grid_size: u32,
    ) -> Result<Option<GameSession>> {
        info!("Finding game session");
        let start = Instant::now();
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        let conn_time = start.elapsed();

        // Get a random game ID from the matchmaking set
        let matchmaking_key = format!(
            "matchmaking:{}:{}:{}",
            single_bet_size, min_players, grid_size
        );

        let game_id: Option<String> = conn.srandmember(&matchmaking_key).await?;
        let pipeline_time = start.elapsed();

        // If we found a game, get its session info
        let session_fetch_start = Instant::now();
        let result = if let Some(game_id) = game_id.as_ref() {
            let key = format!("game_session:{}", game_id);

            let values: Option<Vec<String>> = conn
                .hget(
                    &key,
                    &[
                        "server_id",
                        "single_bet_size",
                        "min_players",
                        "current_players",
                        "grid_size",
                    ],
                )
                .await?;

            if let Some(values) = values {
                if values.len() == 5 {
                    let session = GameSession {
                        game_id: game_id.to_string(),
                        server_id: values[0].clone(),
                        single_bet_size: values[1].parse()?,
                        min_players: values[2].parse()?,
                        current_players: values[3].parse()?,
                        grid_size: values[4].parse()?,
                    };
                    if session.current_players < min_players {
                        Some(session)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let session_fetch_time = session_fetch_start.elapsed();
        let total_time = start.elapsed();

        // Log timing information
        info!(
            found_game = %game_id.is_some(),
            bet_size = %single_bet_size,
            min_players = %min_players,
            grid_size = %grid_size,
            conn_latency_ms = %conn_time.as_millis(),
            pipeline_latency_ms = %pipeline_time.as_millis(),
            session_fetch_latency_ms = %session_fetch_time.as_millis(),
            total_latency_ms = %total_time.as_millis(),
            "Find game session completed"
        );

        if total_time.as_millis() > 500 {
            warn!(
                latency_ms = %total_time.as_millis(),
                "High latency in find_game_session"
            );
        }

        Ok(result)
    }

    // Update player count for a game session
    pub async fn update_player_count(&self, game_id: &str, current_players: u32) -> Result<()> {
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        let key = format!("game_session:{}", game_id);
        let _: () = conn
            .hset(&key, "current_players", current_players.to_string())
            .await?;
        Ok(())
    }

    // Remove a game session when it's finished or aborted
    pub async fn remove_game_session(&self, game_id: &str) -> Result<()> {
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        let mut pipe = redis::pipe();
        pipe.atomic();

        // Get session info first
        let key = format!("game_session:{}", game_id);
        let values: Option<Vec<String>> = conn
            .hget(
                &key,
                &[
                    "server_id",
                    "single_bet_size",
                    "min_players",
                    "current_players",
                    "grid_size",
                ],
            )
            .await?;

        if let Some(values) = values {
            if values.len() == 5 {
                // Remove from matchmaking set
                let matchmaking_key =
                    format!("matchmaking:{}:{}:{}", values[1], values[2], values[4]);
                pipe.srem(matchmaking_key, game_id);
            }
        }

        // Remove session info
        pipe.del(&key);

        // Execute all commands in a single round trip
        let _: () = pipe.query_async(&mut conn).await?;
        Ok(())
    }
}
