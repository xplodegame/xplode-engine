use anyhow::Result;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::{env, time::Instant};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSession {
    pub game_id: String,
    pub region: String,
    pub server_id: String,
    pub single_bet_size: f64,
    pub min_players: u32,
    pub current_players: u32,
}

#[derive(Clone)]
pub struct DiscoveryService {
    redis: redis::Client,
    region: String,
}

impl DiscoveryService {
    pub fn new(redis: redis::Client) -> Self {
        let region = env::var("FLY_REGION").unwrap_or_else(|_| "unknown".to_string());
        Self { redis, region }
    }

    // Register a new game session in the current region
    pub async fn register_game_session(&self, session: GameSession) -> Result<()> {
        let start = Instant::now();
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        let conn_time = start.elapsed();

        // Clone values needed for logging
        let game_id = session.game_id.clone();
        let region = session.region.clone();

        // Store game session info with region
        let key = format!("game_session:{}", session.game_id);
        let mut pipe = redis::pipe();
        pipe.atomic();
        pipe.hset_multiple(
            &key,
            &[
                ("region", session.region.clone()),
                ("server_id", session.server_id.clone()),
                ("single_bet_size", session.single_bet_size.to_string()),
                ("min_players", session.min_players.to_string()),
                ("current_players", session.current_players.to_string()),
            ],
        );

        // Add to region-specific matchmaking set
        let region_matchmaking_key = format!(
            "region_matchmaking:{}:{}:{}",
            session.region, session.single_bet_size, session.min_players
        );
        pipe.sadd(region_matchmaking_key, session.game_id.clone());

        // Add to global matchmaking set
        let matchmaking_key = format!(
            "matchmaking:{}:{}",
            session.single_bet_size, session.min_players
        );
        pipe.sadd(matchmaking_key, session.game_id);

        // Set TTL for cleanup
        pipe.expire(&key, 300);

        // Execute all commands in a single round trip
        let pipeline_start = Instant::now();
        let _: () = pipe.query_async(&mut conn).await?;
        let pipeline_time = pipeline_start.elapsed();
        let total_time = start.elapsed();

        info!(
            game_id = %game_id,
            region = %region,
            conn_latency_ms = %conn_time.as_millis(),
            pipeline_latency_ms = %pipeline_time.as_millis(),
            total_latency_ms = %total_time.as_millis(),
            "Registered game session"
        );

        Ok(())
    }

    // Find best matching game session based on bet size and player count
    pub async fn find_game_session(
        &self,
        single_bet_size: f64,
        min_players: u32,
        preferred_region: Option<String>,
    ) -> Result<Option<GameSession>> {
        info!("Finding game session");
        let start = Instant::now();
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        let conn_time = start.elapsed();

        let mut pipe = redis::pipe();
        pipe.atomic();

        // Add both regional and global search commands to pipeline
        let has_region = preferred_region.is_some();
        if let Some(region) = preferred_region.as_ref() {
            let key = format!(
                "region_matchmaking:{}:{}:{}",
                region, single_bet_size, min_players
            );
            pipe.srandmember(&key);
        }

        let global_matchmaking_key = format!("matchmaking:{}:{}", single_bet_size, min_players);
        pipe.srandmember(&global_matchmaking_key);

        // Execute pipeline to get game IDs
        let pipeline_start = Instant::now();
        let game_ids: Vec<Option<String>> = pipe.query_async(&mut conn).await?;
        let pipeline_time = pipeline_start.elapsed();
        info!("Here 1");

        // Properly handle priority - if we have a region, first ID is regional, second is global
        // If no region specified, we only have global result
        let game_id = if has_region {
            match &game_ids[..] {
                [Some(regional), _] => Some(regional.clone()), // Use regional if available
                [None, global] => global.clone(), // Fall back to global if regional empty
                _ => None,                        // Handle unexpected cases
            }
        } else {
            // No region specified, just use the global result
            game_ids.get(0).and_then(|x| x.clone())
        };

        info!("Here 2");

        // If we found a game, get its session info
        let session_fetch_start = Instant::now();
        let result = if let Some(game_id) = game_id.as_ref() {
            info!("Here 2.1");
            let key = format!("game_session:{}", game_id);
            info!("Key: {}", key);

            // Change this line to handle the pipeline response correctly
            let values: Option<Vec<String>> = conn
                .hget(
                    &key,
                    &[
                        "region",
                        "server_id",
                        "single_bet_size",
                        "min_players",
                        "current_players",
                    ],
                )
                .await?;

            info!("Values: {:?}", values);

            info!("Here 3");

            if let Some(values) = values {
                if values.len() == 5 {
                    let session = GameSession {
                        game_id: game_id.to_string(),
                        region: values[0].clone(),
                        server_id: values[1].clone(),
                        single_bet_size: values[2].parse()?,
                        min_players: values[3].parse()?,
                        current_players: values[4].parse()?,
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
            preferred_region = %preferred_region.unwrap_or_else(|| "none".to_string()),
            bet_size = %single_bet_size,
            min_players = %min_players,
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

    // async fn find_session_in_region(
    //     &self,
    //     conn: &mut MultiplexedConnection,
    //     single_bet_size: f64,
    //     min_players: u32,
    //     region: &str,
    // ) -> Result<Option<GameSession>> {
    //     let region_games: Vec<String> = conn.smembers(format!("region_games:{}", region)).await?;

    //     for game_id in region_games {
    //         if let Some(session) = self.get_game_session(conn, &game_id).await? {
    //             if session.single_bet_size == single_bet_size
    //                 && session.min_players == min_players
    //                 && session.current_players < min_players
    //             {
    //                 return Ok(Some(session));
    //             }
    //         }
    //     }

    //     Ok(None)
    // }

    // async fn get_game_session(
    //     &self,
    //     conn: &mut MultiplexedConnection,
    //     game_id: &str,
    // ) -> Result<Option<GameSession>> {
    //     let key = format!("game_session:{}", game_id);
    //     let values: Option<Vec<String>> = conn
    //         .hget(
    //             &key,
    //             &[
    //                 "region",
    //                 "server_id",
    //                 "single_bet_size",
    //                 "min_players",
    //                 "current_players",
    //             ],
    //         )
    //         .await?;

    //     if let Some(values) = values {
    //         if values.len() == 5 {
    //             return Ok(Some(GameSession {
    //                 game_id: game_id.to_string(),
    //                 region: values[0].clone(),
    //                 server_id: values[1].clone(),
    //                 single_bet_size: values[2].parse()?,
    //                 min_players: values[3].parse()?,
    //                 current_players: values[4].parse()?,
    //             }));
    //         }
    //     }

    //     Ok(None)
    // }

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
                    "region",
                    "server_id",
                    "single_bet_size",
                    "min_players",
                    "current_players",
                ],
            )
            .await?;

        if let Some(values) = values {
            if values.len() == 5 {
                // Remove from region-specific matchmaking set
                let region_matchmaking_key = format!(
                    "region_matchmaking:{}:{}:{}",
                    values[0], values[2], values[3]
                );
                pipe.srem(region_matchmaking_key, game_id);

                // Remove from global matchmaking set
                let matchmaking_key = format!("matchmaking:{}:{}", values[2], values[3]);
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
