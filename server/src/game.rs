use anyhow::Result;
use common::{
    db::{self, establish_connection},
    utils::Currency,
};
use futures_util::{
    lock::Mutex,
    stream::{SplitSink, StreamExt},
    SinkExt,
};

use redis::{AsyncCommands, Client};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        broadcast::{self},
        mpsc, RwLock,
    },
};
use tokio_websockets::{Message, ServerBuilder, WebSocketStream};
use tracing::info;

use uuid::Uuid;

use crate::{board::Board, player::Player};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameState {
    WAITING {
        game_id: String,
        creator: Player,
        board: Board,
        single_bet_size: f64,
        min_players: u32,
        players: Vec<Player>,
    },
    RUNNING {
        game_id: String,
        players: Vec<Player>,
        board: Board,
        turn_idx: usize,
        single_bet_size: f64,
        locks: Option<Vec<(usize, usize)>>,
    },
    FINISHED {
        game_id: String,
        loser_idx: usize,
        board: Board,
        players: Vec<Player>,
        single_bet_size: f64,
    },
    // During the start, user doesn't make a move for some predefined time
    ABORTED {
        game_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameMessage {
    Play {
        player_id: String,
        single_bet_size: f64,
        min_players: u32,
        bombs: u32,
        grid: u32,
    },
    Join {
        game_id: String,
        player_id: String,
    },
    MakeMove {
        game_id: String,
        x: usize,
        y: usize,
    },
    Lock {
        x: usize,
        y: usize,
        game_id: String,
    },
    LockComplete {
        game_id: String,
    },
    Stop {
        game_id: String,
        abort: bool,
    },
    Ping {
        game_id: Option<String>,
        player_id: Option<String>,
    },
    GameUpdate(GameState),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameMessageWrapper {
    server_id: String,
    game_message: GameMessage,
}
#[derive(Clone)]
pub struct GameRegistry {
    games: Arc<RwLock<HashMap<String, GameState>>>,
    active_players: Arc<RwLock<HashSet<String>>>,
    // Fixme: Take care of this Arc
    game_channels: Arc<RwLock<HashMap<String, Arc<mpsc::Sender<GameMessage>>>>>,
    broadcast_channels: Arc<RwLock<HashMap<String, broadcast::Sender<GameMessage>>>>,
    // player_streams: Arc<
    //     RwLock<HashMap<String, Vec<Arc<Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>>>>,
    // >,
    redis: redis::Client,
}

type WebSocketSink = SplitSink<WebSocketStream<TcpStream>, Message>;

impl GameRegistry {
    // Only save game state to Redis for critical state changes
    pub async fn save_game_state(&self, game_id: String, state: GameState) {
        info!("*********************Saving game state to Redis*********************");
        // Don't block the main flow - fire and forget
        tokio::spawn({
            let redis = self.redis.clone();
            let game_id = game_id.clone();
            let state_json = serde_json::to_string(&state).unwrap();

            async move {
                if let Ok(mut conn) = redis.get_multiplexed_async_connection().await {
                    let res = conn
                        .set_ex::<String, String, ()>(game_id, state_json, 300)
                        .await;
                    info!("Redis set_ex result: {:?}", res);
                }
            }
        });
    }

    // Only load game states from Redis during startup or when absolutely needed
    pub async fn load_game_state(&self) {
        let mut conn = match self.redis.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(_) => return, // Fail silently and rely on in-memory state
        };

        let keys: Vec<String> = match conn.keys("*").await {
            Ok(keys) => keys,
            Err(_) => return,
        };

        for key in keys {
            if let Ok(state) = conn.get::<_, String>(key.clone()).await {
                if let Ok(game_state) = serde_json::from_str::<GameState>(&state) {
                    self.games.write().await.insert(key, game_state);
                }
            }
        }
    }

    // Prioritize in-memory state, only fall back to Redis when necessary
    pub async fn get_game_state(&self, game_id: &str) -> Option<GameState> {
        // First check in-memory cache
        let games_read = self.games.read().await;
        if let Some(state) = games_read.get(game_id) {
            return Some(state.clone());
        }
        drop(games_read);

        // Only if not in memory, try Redis once
        let mut conn = match self.redis.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(_) => return None,
        };

        let redis_value: Option<String> = redis::cmd("GET")
            .arg(game_id)
            .query_async(&mut conn)
            .await
            .ok();

        if let Some(state_json) = redis_value {
            if let Ok(game_state) = serde_json::from_str::<GameState>(&state_json) {
                // Update in-memory cache
                self.games
                    .write()
                    .await
                    .insert(game_id.to_string(), game_state.clone());
                return Some(game_state);
            }
        }
        None
    }

    // Only load from Redis if absolutely necessary
    pub async fn load_game_if_absent(&self, game_id: &str) {
        // Check if already in memory first
        let games_read = self.games.read().await;
        if games_read.get(game_id).is_some() {
            return;
        }
        drop(games_read);

        // Only try Redis once
        let mut conn = match self.redis.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(_) => return,
        };

        let redis_value: Option<String> = redis::cmd("GET")
            .arg(game_id)
            .query_async(&mut conn)
            .await
            .ok();

        if let Some(state_json) = redis_value {
            if let Ok(game_state) = serde_json::from_str::<GameState>(&state_json) {
                self.games
                    .write()
                    .await
                    .insert(game_id.to_string(), game_state);
            }
        }
    }

    pub async fn subscribe_to_channel(
        &self,
        server_id: String,
        channel: String,
        ws_write: Arc<Mutex<WebSocketSink>>,
    ) -> Result<()> {
        info!("SUbscribed to channel: {:?}", channel);
        // Channel name should only rely on game_id
        let mut broadcast_channels = self.broadcast_channels.write().await;

        if broadcast_channels.get(&channel).is_none() {
            let (tx, _rx) = broadcast::channel(100);
            broadcast_channels.insert(channel.clone(), tx.clone());
            info!("Inserting into broadcast channel");

            // Spawn a single Redis subscription for this game_id
            let redis_client = self.redis.clone();

            let tx_clone = tx.clone();
            let registry_clone = self.clone();

            let channel = channel.clone();
            tokio::spawn(async move {
                let mut pubsub = redis_client.get_async_pubsub().await.unwrap();
                pubsub.subscribe(&channel).await.unwrap();
                let mut stream = pubsub.on_message();

                while let Some(msg) = stream.next().await {
                    if let Ok(payload) = msg.get_payload::<String>() {
                        if let Ok(game_message_wrapper) =
                            serde_json::from_str::<GameMessageWrapper>(&payload)
                        {
                            // Do not broadcast if it is from the same server id
                            if game_message_wrapper.server_id != server_id {
                                // Broadcast Redis message to all local clients
                                let _ = tx_clone.send(game_message_wrapper.game_message.clone());

                                // Re-publish to Redis for redundancy
                                registry_clone
                                    .publish_message(channel.clone(), game_message_wrapper, true)
                                    .await?;
                            }
                        }
                    }
                }
                anyhow::Ok(())
            });
        }
        info!("################Broadcasting now\n");
        let broadcast_tx = broadcast_channels.get(&channel).unwrap();
        let mut broadcast_rx = broadcast_tx.subscribe();

        info!(
            "broadcast_tx.receiver_count(): {}",
            broadcast_tx.receiver_count()
        );
        tokio::spawn(async move {
            while let Ok(game_message) = broadcast_rx.recv().await {
                info!("Got broadcast message");
                let mut ws_sink = ws_write.lock().await;
                if ws_sink
                    .send(Message::binary(serde_json::to_vec(&game_message).unwrap()))
                    .await
                    .is_err()
                {
                    eprintln!("Player disconnected");
                }
            }
        });
        Ok(())
    }

    // ✅ Publish messages using multiplexed connection
    pub async fn publish_message(
        &self,
        channel: String, // channel == game_id
        game_message_wrapper: GameMessageWrapper,
        from_redis: bool,
    ) -> Result<()> {
        info!("***********publishing message***********");
        // Send to local clients
        if let Some(channel) = self.broadcast_channels.read().await.get(&channel) {
            let _ = channel.send(game_message_wrapper.game_message.clone());
        }

        // Publish to Redis only if not from Redis
        if !from_redis {
            info!("Publishing to Redis");
            let mut conn = self.redis.get_multiplexed_async_connection().await.unwrap();
            conn.publish::<String, String, ()>(
                channel,
                serde_json::to_string(&game_message_wrapper).unwrap(),
            )
            .await?;
        }
        Ok(())
    }

    // Add new cleanup method
    pub async fn cleanup_player(&self, player_id: &str) {
        // Remove from active players
        let mut active_players_write = self.active_players.write().await;
        active_players_write.remove(player_id);

        // Check if player is in any WAITING games and clean those up
        let mut games_write = self.games.write().await;
        let mut games_to_abort = Vec::new();

        for (game_id, state) in games_write.iter() {
            if let GameState::WAITING { creator, .. } = state {
                if creator.id == player_id {
                    games_to_abort.push(game_id.clone());
                }
            }
        }

        // Abort any WAITING games where this player was the creator
        for game_id in games_to_abort {
            let aborted_state = GameState::ABORTED {
                game_id: game_id.clone(),
            };
            games_write.insert(game_id.clone(), aborted_state.clone());

            // Clean up in Redis asynchronously
            tokio::spawn({
                let redis = self.redis.clone();
                let game_id = game_id.clone();
                let state_json = serde_json::to_string(&aborted_state).unwrap();

                async move {
                    if let Ok(mut conn) = redis.get_multiplexed_async_connection().await {
                        let _ = conn
                            .set_ex::<String, String, ()>(game_id, state_json, 300)
                            .await;
                    }
                }
            });
        }
    }
}

pub struct GameServer {
    server_id: String,
    registry: GameRegistry,
}

impl GameServer {
    pub async fn new() -> Self {
        let redis_url = env::var("REDIS_URL").unwrap();
        info!("Redis URL: {}", redis_url);
        let redis_client = Client::open(redis_url).unwrap();

        Self {
            server_id: Uuid::new_v4().to_string(),
            registry: GameRegistry {
                games: Arc::new(RwLock::new(HashMap::new())),
                active_players: Arc::new(RwLock::new(HashSet::new())),
                game_channels: Arc::new(RwLock::new(HashMap::new())),
                broadcast_channels: Arc::new(RwLock::new(HashMap::new())),
                // player_streams: Arc::new(RwLock::new(HashMap::new())),
                redis: redis_client,
            },
        }
    }

    pub async fn start(&self, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        info!("Server listening on {}", addr);

        while let std::result::Result::Ok((stream, _)) = listener.accept().await {
            let registry = self.registry.clone();
            let server_id = self.server_id.clone();

            tokio::spawn(async move {
                info!("Establishing connection");
                if let Err(e) = GameServer::handle_connection(server_id, registry, stream).await {
                    eprintln!("Error handling connection: {}", e);
                }
            });
        }

        Ok(())
    }

    async fn handle_connection(
        server_id: String,
        registry: GameRegistry,
        stream: TcpStream,
    ) -> anyhow::Result<()> {
        let ws_stream = ServerBuilder::new().accept(stream).await?;
        let pool = establish_connection().await;

        let (ws_write, mut ws_read) = ws_stream.split();

        let ws_write = Arc::new(Mutex::new(ws_write));

        // Create a channel for this game connection
        let (server_tx, mut server_rx) = tokio::sync::mpsc::channel(500);
        let server_tx = Arc::new(server_tx);

        // Keep track of the current player_id for cleanup
        let current_player_id = Arc::new(RwLock::new(String::new()));
        let registry_clone = registry.clone();

        // Spawn a task to handle incoming WebSocket messages
        let _ = tokio::spawn({
            let server_tx = server_tx.clone();
            let current_player_id = current_player_id.clone();
            async move {
                while let Some(msg) = ws_read.next().await {
                    info!("Incoming msg");
                    let server_tx_inner = server_tx.clone();

                    match msg {
                        Ok(message) => {
                            let current_player_id = current_player_id.clone();
                            tokio::spawn(async move {
                                match serde_json::from_slice(message.as_payload()) {
                                    Ok(game_msg) => {
                                        info!("msg: {:?}", game_msg);
                                        // Update current_player_id if this is a Play or Join message
                                        if let GameMessage::Play { player_id, .. } = &game_msg {
                                            *current_player_id.write().await = player_id.clone();
                                        } else if let GameMessage::Join { player_id, .. } =
                                            &game_msg
                                        {
                                            *current_player_id.write().await = player_id.clone();
                                        }
                                        if let Err(e) = server_tx_inner.send(game_msg).await {
                                            eprintln!("Error sending message: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Deserialization error: {}", e);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("WebSocket error: {}", e);
                            break;
                        }
                    }
                }

                // WebSocket connection closed - clean up the player
                let player_id = current_player_id.read().await.clone();
                if !player_id.is_empty() {
                    registry_clone.cleanup_player(&player_id).await;
                }
            }
        });
        // Process game messages
        while let Some(message) = server_rx.recv().await {
            match message {
                GameMessage::Ping { game_id, player_id } => {
                    if let Some(game_id) = game_id {
                        registry
                            .subscribe_to_channel(server_id.clone(), game_id, ws_write.clone())
                            .await?;
                    }

                    if let Some(player_id) = player_id {
                        let mut active_players_write = registry.active_players.write().await;
                        active_players_write.insert(player_id);
                    }
                    let response = "Pong".to_string();
                    if let Err(e) = ws_write
                        .lock()
                        .await
                        .send(Message::binary(serde_json::to_vec(&response)?))
                        .await
                    {
                        eprintln!("Error sending GameUpdate message: {}", e);
                    }
                }
                GameMessage::Play {
                    player_id,
                    single_bet_size,
                    min_players,
                    bombs,
                    grid,
                } => {
                    info!("Play request");
                    let active_players_read = registry.active_players.read().await;

                    if active_players_read.contains(&player_id) {
                        let response =
                            GameMessage::Error("You are already waiting for a game".to_string());
                        ws_write
                            .lock()
                            .await
                            .send(Message::binary(serde_json::to_vec(&response)?))
                            .await?;
                        continue;
                    }
                    drop(active_players_read);
                    let games_read = registry.games.read().await;

                    //TODO: Think of a better way to do this
                    let mut matched_game = games_read.iter().find_map(|(game_id, state)| {
                        if let GameState::WAITING {
                            single_bet_size: size,
                            min_players: mp,
                            ..
                        } = state
                        {
                            if *size == single_bet_size && *mp == min_players {
                                Some((game_id.clone(), state.clone()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });
                    drop(games_read);
                    if matched_game.is_none() {
                        info!("Loading game from memory");
                        // try to match game from redis

                        registry.load_game_state().await;

                        // FIXME: Remove redundant checking
                        let games_read = registry.games.read().await;

                        info!("Keys length after: {:?}", games_read.keys().len());
                        matched_game = games_read.iter().find_map(|(game_id, state)| {
                            if let GameState::WAITING {
                                single_bet_size: size,
                                min_players: mp,
                                ..
                            } = state
                            {
                                if *size == single_bet_size && *mp == min_players {
                                    Some((game_id.clone(), state.clone()))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        });
                        drop(games_read);
                    }

                    if let Some((
                        game_id,
                        GameState::WAITING {
                            creator,
                            board,
                            single_bet_size,
                            min_players,
                            mut players,
                            ..
                        },
                    )) = matched_game
                    {
                        // Now we can safely create a new player and prepare the new game state
                        info!("Game has begun");
                        let player = Player::new(player_id.clone());
                        players.push(player);

                        let new_game_state = if players.len() < min_players as usize {
                            GameState::WAITING {
                                game_id: game_id.clone(),
                                creator: creator.clone(),
                                board: board.clone(),
                                single_bet_size,
                                min_players,
                                players: players.clone(),
                            }
                        } else {
                            GameState::RUNNING {
                                game_id: game_id.clone(),
                                players: players.clone(),
                                board: board.clone(),
                                turn_idx: 0,
                                single_bet_size,
                                locks: None,
                            }
                        };

                        let mut games_write = registry.games.write().await;
                        games_write.insert(game_id.clone(), new_game_state.clone());

                        // asyncrhonously save the state in redis
                        registry
                            .save_game_state(game_id.clone(), new_game_state.clone())
                            .await;

                        // need to subscribe so that this stream sink could receive messages
                        registry
                            .subscribe_to_channel(
                                server_id.clone(),
                                game_id.clone(),
                                ws_write.clone(),
                            )
                            .await?;
                        let mut game_channels_write = registry.game_channels.write().await;
                        game_channels_write.insert(game_id.clone(), server_tx.clone());
                        drop(game_channels_write);

                        let game_message = GameMessage::GameUpdate(new_game_state.clone());

                        let wrapper = GameMessageWrapper {
                            server_id: server_id.clone(),
                            game_message,
                        };

                        registry
                            .publish_message(game_id.clone(), wrapper, false)
                            .await?;
                        let mut active_players_write = registry.active_players.write().await;
                        active_players_write.insert(player_id);
                    } else {
                        info!("User will create a game");
                        let game_id = Uuid::new_v4().to_string();
                        let board = Board::new(grid as usize, bombs as usize);
                        let player = Player::new(player_id.clone());

                        let game_state = GameState::WAITING {
                            game_id: game_id.clone(),
                            creator: player.clone(),
                            board,
                            single_bet_size,
                            min_players,
                            players: vec![player.clone()],
                        };

                        // Store game state and channel
                        let mut games_write = registry.games.write().await;
                        games_write.insert(game_id.clone(), game_state.clone());

                        // asyncrhonously save the state in redis
                        registry
                            .save_game_state(game_id.clone(), game_state.clone())
                            .await;

                        let mut game_channels_write = registry.game_channels.write().await;
                        game_channels_write.insert(game_id.clone(), server_tx.clone());
                        drop(game_channels_write);

                        // need to subscribe so that this stream sink could receive messages
                        registry
                            .subscribe_to_channel(
                                server_id.clone(),
                                game_id.clone(),
                                ws_write.clone(),
                            )
                            .await?;
                        let game_message = GameMessage::GameUpdate(game_state.clone());

                        let wrapper = GameMessageWrapper {
                            server_id: server_id.clone(),
                            game_message,
                        };

                        registry
                            .publish_message(game_id.clone(), wrapper, false)
                            .await?;
                        let mut active_players_write = registry.active_players.write().await;
                        active_players_write.insert(player_id);
                    }
                }
                GameMessage::Join { game_id, player_id } => {
                    info!("Request to join:: {:?} game", game_id);

                    // let games_read = registry.games.read().await;
                    // let game_state = games_read.get(&game_id);
                    let game_state = registry.get_game_state(&game_id).await;
                    if let Some(GameState::WAITING {
                        game_id,
                        creator,
                        board,
                        single_bet_size,
                        min_players,
                        players,
                    }) = game_state
                    {
                        let new_player = Player::new(player_id.clone());
                        let mut players = players.clone();
                        players.push(new_player);

                        let new_game_state = if players.len() < min_players as usize {
                            GameState::WAITING {
                                game_id: game_id.clone(),
                                creator: creator.clone(),
                                board: board.clone(),
                                single_bet_size,
                                min_players,
                                players,
                            }
                        } else {
                            GameState::RUNNING {
                                game_id: game_id.clone(),
                                players,
                                board: board.clone(),
                                turn_idx: 0,
                                single_bet_size,
                                locks: None,
                            }
                        };
                        let mut games_write = registry.games.write().await;
                        games_write.insert(game_id.clone(), new_game_state.clone());

                        // asyncrhonously save the state in redis
                        registry
                            .save_game_state(game_id.clone(), new_game_state.clone())
                            .await;

                        // need to subscribe so that this stream sink could receive messages
                        registry
                            .subscribe_to_channel(
                                server_id.clone(),
                                game_id.clone(),
                                ws_write.clone(),
                            )
                            .await?;

                        let game_message = GameMessage::GameUpdate(new_game_state.clone());

                        let wrapper = GameMessageWrapper {
                            server_id: server_id.clone(),
                            game_message,
                        };

                        registry
                            .publish_message(game_id.clone(), wrapper, false)
                            .await?;
                        let mut active_players_write = registry.active_players.write().await;
                        active_players_write.insert(player_id);
                    } else {
                        let response =
                            GameMessage::Error("this game is not accepting players".to_string());
                        if let Err(err) = ws_write
                            .lock()
                            .await
                            .send(Message::binary(serde_json::to_vec(&response)?))
                            .await
                        {
                            eprintln!("Failed to send error message to the client:: {:?}", err);
                        }
                    }
                }
                GameMessage::Stop { game_id, abort } => {
                    registry.load_game_if_absent(&game_id).await;

                    let mut games_write = registry.games.write().await;
                    if !abort {
                        // Meaning other players won
                        if let Some(game_state) = games_write.get_mut(&game_id) {
                            if let GameState::RUNNING {
                                players,
                                board,
                                turn_idx,
                                single_bet_size,
                                ..
                            } = game_state
                            {
                                info!("Hello about to stop the game**************************************");
                                let loser = turn_idx;
                                let new_game_state = GameState::FINISHED {
                                    game_id: game_id.clone(),
                                    loser_idx: *loser,
                                    board: board.clone(),
                                    players: players.clone(),
                                    single_bet_size: *single_bet_size,
                                };

                                // remove players from active state
                                let mut active_players_write =
                                    registry.active_players.write().await;

                                let ids = players.iter().map(|p| p.id.clone()).collect::<Vec<_>>();

                                active_players_write.retain(|x| !ids.contains(x));
                                // asyncrhonously save the state in redis
                                registry
                                    .save_game_state(game_id.clone(), new_game_state.clone())
                                    .await;

                                // UPDATING THE DB AS WELL HERE
                                let winning_amount =
                                    *single_bet_size / ((players.clone().len() - 1) as f64);

                                let user_ids: Vec<i32> = players
                                    .iter()
                                    .map(|p| p.id.parse::<i32>().unwrap())
                                    .collect();
                                db::update_player_balances(
                                    &pool,
                                    &user_ids,
                                    *loser,
                                    *single_bet_size,
                                    winning_amount,
                                    Currency::MON,
                                )
                                .await?;
                                *game_state = new_game_state;
                                let game_message = GameMessage::GameUpdate(game_state.clone());

                                let wrapper = GameMessageWrapper {
                                    server_id: server_id.clone(),
                                    game_message,
                                };

                                registry
                                    .publish_message(game_id.clone(), wrapper, false)
                                    .await?;
                            }
                        }
                    } else if let Some(game_state) = games_write.get_mut(&game_id) {
                        if let GameState::RUNNING { players, .. } = game_state {
                            // remove players from active state
                            let mut active_players_write = registry.active_players.write().await;

                            let ids = players.iter().map(|p| p.id.clone()).collect::<Vec<_>>();

                            active_players_write.retain(|x| !ids.contains(x));
                        }
                        *game_state = GameState::ABORTED {
                            game_id: game_id.clone(),
                        };
                        let game_message = GameMessage::GameUpdate(game_state.clone());

                        let wrapper = GameMessageWrapper {
                            server_id: server_id.clone(),
                            game_message,
                        };

                        registry
                            .publish_message(game_id.clone(), wrapper, false)
                            .await?;
                    }
                }
                GameMessage::MakeMove { game_id, x, y } => {
                    registry.load_game_if_absent(&game_id).await;
                    let mut games_write = registry.games.write().await;

                    if let Some(game_state) = games_write.get_mut(&game_id) {
                        match game_state {
                            GameState::RUNNING {
                                players,
                                board,
                                turn_idx,
                                single_bet_size,
                                locks,
                                ..
                            } => {
                                let game_ended = board.mine(x, y);

                                // Clone everything we need before any modifications
                                let players_clone = players.clone();
                                let turn_idx_clone = *turn_idx;
                                let single_bet_size_clone = *single_bet_size;

                                if game_ended {
                                    let new_game_state = GameState::FINISHED {
                                        game_id: game_id.clone(),
                                        loser_idx: turn_idx_clone,
                                        board: board.clone(),
                                        players: players_clone.clone(),
                                        single_bet_size: single_bet_size_clone,
                                    };
                                    *game_state = new_game_state.clone();

                                    // Async DB operations
                                    let winning_amount =
                                        single_bet_size_clone / ((players_clone.len() - 1) as f64);
                                    let user_ids: Vec<i32> = players_clone
                                        .iter()
                                        .map(|p| p.id.parse::<i32>().unwrap())
                                        .collect();

                                    // remove players from active state
                                    let mut active_players_write =
                                        registry.active_players.write().await;

                                    let ids = players_clone
                                        .iter()
                                        .map(|p| p.id.clone())
                                        .collect::<Vec<_>>();

                                    active_players_write.retain(|x| !ids.contains(x));

                                    registry
                                        .save_game_state(game_id.clone(), new_game_state)
                                        .await;

                                    let pool_clone = pool.clone();
                                    tokio::spawn(async move {
                                        let _ = db::update_player_balances(
                                            &pool_clone,
                                            &user_ids,
                                            turn_idx_clone,
                                            single_bet_size_clone,
                                            winning_amount,
                                            Currency::MON,
                                        )
                                        .await;
                                    });
                                } else {
                                    // Not needed here as they will be updated in lock complete
                                    // *turn_idx = (*turn_idx + 1) % players.len();
                                    *locks = None;
                                }

                                // Broadcast the update for both cases
                                let game_message = GameMessage::GameUpdate(game_state.clone());
                                let wrapper = GameMessageWrapper {
                                    server_id: server_id.clone(),
                                    game_message,
                                };
                                drop(games_write);
                                registry
                                    .publish_message(game_id.clone(), wrapper, false)
                                    .await?;
                            }
                            _ => {
                                // Invalid game state for move
                                ws_write
                                    .lock()
                                    .await
                                    .send(Message::binary(serde_json::to_vec(
                                        &GameMessage::Error(
                                            "Cannot make move in current game state".to_string(),
                                        ),
                                    )?))
                                    .await?;
                            }
                        }
                    }
                }
                GameMessage::Lock { x, y, game_id } => {
                    let mut games_write = registry.games.write().await;

                    if let Some(game_state) = games_write.get_mut(&game_id) {
                        if let GameState::RUNNING { locks, .. } = game_state {
                            let locks = locks.get_or_insert_with(Vec::new);
                            locks.push((x, y));
                            // Don't save to Redis for lock updates - they're temporary
                        }

                        // Just broadcast the update
                        let game_message = GameMessage::GameUpdate(game_state.clone());
                        let wrapper = GameMessageWrapper {
                            server_id: server_id.clone(),
                            game_message,
                        };

                        registry
                            .publish_message(game_id.clone(), wrapper.clone(), false)
                            .await?;
                    }
                }
                GameMessage::LockComplete { game_id } => {
                    let mut games_write = registry.games.write().await;

                    if let Some(game_state) = games_write.get_mut(&game_id) {
                        if let GameState::RUNNING {
                            game_id,
                            turn_idx,
                            players,
                            ..
                        } = game_state
                        {
                            *turn_idx = (*turn_idx + 1) % players.len();

                            // Save turn changes to Redis - this is important for game state
                            registry
                                .save_game_state(game_id.clone(), game_state.clone())
                                .await;
                        }

                        let game_message = GameMessage::GameUpdate(game_state.clone());
                        let wrapper = GameMessageWrapper {
                            server_id: server_id.clone(),
                            game_message,
                        };

                        registry
                            .publish_message(game_id.clone(), wrapper.clone(), false)
                            .await?;
                    }
                }
                GameMessage::GameUpdate(msg) => {
                    // unreachable!("Should fail if execution enters here");
                    let game_message = GameMessage::GameUpdate(msg.clone());

                    let wrapper = GameMessageWrapper {
                        server_id: server_id.clone(),
                        game_message,
                    };
                    info!("Inside game update");
                    match msg {
                        GameState::RUNNING { game_id, .. } => {
                            registry
                                .publish_message(game_id.clone(), wrapper, false)
                                .await?;
                        }
                        GameState::FINISHED {
                            game_id,
                            loser_idx,
                            players,
                            single_bet_size,
                            ..
                        } => {
                            registry
                                .publish_message(game_id.clone(), wrapper, false)
                                .await?;
                            // Update the db
                            let winning_amount = single_bet_size / ((players.len() - 1) as f64);

                            let user_ids: Vec<i32> = players
                                .iter()
                                .map(|p| p.id.parse::<i32>().unwrap())
                                .collect();
                            db::update_player_balances(
                                &pool,
                                &user_ids,
                                loser_idx,
                                single_bet_size,
                                winning_amount,
                                Currency::MON,
                            )
                            .await?;
                        }
                        GameState::ABORTED { game_id } => {
                            registry
                                .publish_message(game_id.clone(), wrapper, false)
                                .await?;
                        }
                        GameState::WAITING { game_id, .. } => {
                            registry
                                .publish_message(game_id.clone(), wrapper, false)
                                .await?;
                        }
                    }
                }
                _ => {}
            }
        }

        // let _ = tokio::try_join!(readers_task, writers_task);
        Ok(())
    }
}

// info!("Game update here");

// if let GameState::FINISHED {
//     loser_idx,
//     players,
//     single_bet_size,
//     ..
// } = msg
// {
//     // Update the db
//     let winning_amount = single_bet_size / ((players.len() - 1) as f64);

//     let user_ids: Vec<u32> = players
//         .iter()
//         .map(|p| p.id.parse::<u32>().unwrap())
//         .collect();
//     db::update_player_balances(
//         &pool,
//         &user_ids,
//         loser_idx,
//         single_bet_size,
//         winning_amount,
//     )
//     .await?;
// }
