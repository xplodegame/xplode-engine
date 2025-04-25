use anyhow::Result;
use common::{
    db::{self, establish_connection},
    telegram::send_telegram_message,
    utils::Currency,
};
use futures_util::{
    lock::Mutex,
    stream::{SplitSink, StreamExt},
    SinkExt,
};

use http::HeaderValue;
use redis::Client;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, sync::Arc};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::{
        broadcast::{self},
        mpsc, RwLock,
    },
};
use tokio_websockets::{Message, ServerBuilder, WebSocketStream};
use tracing::{error, info};

use uuid::Uuid;

use crate::{
    board::Board,
    discovery::{DiscoveryService, GameSession},
    player::Player,
};

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
    REMATCH {
        game_id: String,
        players: Vec<Player>,
        board: Board,
        single_bet_size: f64,
        accepted: Vec<usize>,
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
        name: String,
        single_bet_size: f64,
        min_players: u32,
        bombs: u32,
        grid: u32,
    },
    Join {
        game_id: String,
        player_id: String,
        name: String,
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
    RedirectToServer {
        game_id: String,
        machine_id: String,
    },
    Rematch {
        game_id: String,
        player_id: String,
    },
    RematchRequest {
        game_id: String,
        requester: String,
    },
    RematchResponse {
        game_id: String,
        player_id: String,
        want_rematch: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameMessageWrapper {
    server_id: String,
    game_message: GameMessage,
}
#[derive(Clone)]
pub struct GameRegistry {
    games: Arc<RwLock<HashMap<String, GameState>>>,
    active_players: Arc<RwLock<HashMap<String, String>>>,
    game_channels: Arc<RwLock<HashMap<String, Arc<mpsc::Sender<GameMessage>>>>>,
    broadcast_channels: Arc<RwLock<HashMap<String, broadcast::Sender<GameMessage>>>>,
    // redis: redis::Client,
    discovery: DiscoveryService,
    server_id: String,
}

type WebSocketSink = SplitSink<WebSocketStream<TcpStream>, Message>;

impl GameRegistry {
    pub fn new(redis: redis::Client, server_id: String) -> Self {
        Self {
            games: Arc::new(RwLock::new(HashMap::new())),
            active_players: Arc::new(RwLock::new(HashMap::new())),
            game_channels: Arc::new(RwLock::new(HashMap::new())),
            broadcast_channels: Arc::new(RwLock::new(HashMap::new())),
            // redis: redis.clone(),
            discovery: DiscoveryService::new(redis),
            server_id,
        }
    }

    pub async fn save_game_state(&self, game_id: String, state: GameState) {
        match &state {
            GameState::RUNNING { players, .. } => {
                // Update discovery service with current player count
                let _ = self
                    .discovery
                    .update_player_count(&game_id, players.len() as u32)
                    .await;
            }
            GameState::FINISHED { .. } | GameState::ABORTED { .. } => {
                // Remove from discovery when game ends
                let _ = self.discovery.remove_game_session(&game_id).await;
            }
            _ => {}
        }
    }

    pub async fn get_game_state(&self, game_id: &str) -> Option<GameState> {
        // Only check in-memory state since we don't store in Redis anymore
        let games_read = self.games.read().await;
        info!("Game keys: {:?}", games_read.keys().len());
        games_read.get(game_id).cloned()
    }

    // This is still needed for real-time game updates between players
    pub async fn subscribe_to_channel(
        &self,
        _server_id: String, // Not needed anymore since we're local only
        channel: String,
        ws_write: Arc<Mutex<WebSocketSink>>,
    ) -> Result<()> {
        info!("Subscribing to channel: {:?}", channel);
        let mut broadcast_channels = self.broadcast_channels.write().await;

        // Create a new broadcast channel if it doesn't exist
        if broadcast_channels.get(&channel).is_none() {
            let (tx, _rx) = broadcast::channel(100);
            broadcast_channels.insert(channel.clone(), tx);
        }

        // Get the sender and create a new receiver
        let broadcast_tx = broadcast_channels.get(&channel).unwrap();
        let mut broadcast_rx = broadcast_tx.subscribe();
        drop(broadcast_channels); // Release the write lock

        // Spawn a task to forward messages to this client's WebSocket
        tokio::spawn(async move {
            while let Ok(game_message) = broadcast_rx.recv().await {
                let mut ws_sink = ws_write.lock().await;
                if ws_sink
                    .send(Message::binary(serde_json::to_vec(&game_message).unwrap()))
                    .await
                    .is_err()
                {
                    eprintln!("Player disconnected");
                    break; // Exit the loop if client disconnects
                }
            }
        });

        Ok(())
    }

    // Simplified publish method for local broadcasting only
    pub async fn publish_message(
        &self,
        channel: String,
        game_message_wrapper: GameMessageWrapper,
        _from_redis: bool, // Not needed anymore since we're local only
    ) -> Result<()> {
        info!("--------------------------------");
        info!("Publishing message to channel: {:?}", channel);
        info!("--------------------------------");
        if let Some(broadcast_tx) = self.broadcast_channels.read().await.get(&channel) {
            info!("--------------------------------");
            info!("Sending message to channel: {:?}", channel);
            info!("--------------------------------");
            let _ = broadcast_tx.send(game_message_wrapper.game_message);
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
            games_write.insert(game_id.clone(), aborted_state);

            // Only remove from discovery service, no need to save state
            let _ = self.discovery.remove_game_session(&game_id).await;
        }
    }

    // Modify the matchmaking logic in handle_play_message
    async fn handle_play_message(
        &self,
        player_id: String,
        name: String,
        single_bet_size: f64,
        min_players: u32,
        bombs: u32,
        grid: u32,
    ) -> Result<Option<GameState>> {
        info!("Handling play message");
        // First check if player is already in a game
        let active_players_read = self.active_players.read().await;
        if active_players_read.contains_key(&player_id) {
            return Ok(None);
        }
        drop(active_players_read);

        // Try to find an existing game session through discovery service
        // let current_region = env::var("FLY_REGION").unwrap_or_else(|_| "unknown".to_string());
        if let Some(session) = self
            .discovery
            .find_game_session(single_bet_size, min_players, grid)
            .await?
        {
            // If the session is on this server, get it from local state
            if session.server_id == self.server_id {
                let state = {
                    let games_read = self.games.read().await;
                    if let Some(state) = games_read.get(&session.game_id) {
                        Some(state.clone())
                    } else {
                        None
                    }
                };

                if let Some(GameState::WAITING {
                    game_id,
                    creator,
                    board,
                    single_bet_size,
                    min_players,
                    mut players,
                }) = state
                {
                    let player = Player::new(player_id.clone(), name.clone());
                    players.push(player);

                    // Update player count in Redis
                    self.discovery
                        .update_player_count(&game_id, players.len() as u32)
                        .await?;

                    let new_state = if players.len() < min_players as usize {
                        GameState::WAITING {
                            game_id: game_id.clone(),
                            creator,
                            board,
                            single_bet_size,
                            min_players,
                            players,
                        }
                    } else {
                        // Game is transitioning to RUNNING state
                        // Remove from discovery since it's no longer accepting players
                        self.discovery.remove_game_session(&game_id).await?;

                        GameState::RUNNING {
                            game_id: game_id.clone(),
                            players,
                            board,
                            turn_idx: 0,
                            single_bet_size,
                            locks: None,
                        }
                    };

                    let mut games_write = self.games.write().await;
                    games_write.insert(game_id.clone(), new_state.clone());
                    return Ok(Some(new_state));
                }
            }
            // If session is on another server, return None - client should reconnect to that server
            return Ok(None);
        }

        // Create new game if no suitable session found
        let game_id = Uuid::new_v4().to_string();
        let board = Board::new(grid as usize, bombs as usize);
        let player = Player::new(player_id.clone(), name.clone());

        let game_state = GameState::WAITING {
            game_id: game_id.clone(),
            creator: player.clone(),
            board,
            single_bet_size,
            min_players,
            players: vec![player.clone()],
        };

        info!("--------------------------------");
        info!("Ahoy");
        info!("--------------------------------");

        if env::var("ENVIRONMENT").unwrap_or_else(|_| "production".to_string()) == "production" {
            // Send Telegram notification.
            let game_url = format!("https://playxplode.xyz/multiplayer/{}", game_id);
            let notification_message = format!(
            "🎮 New game created!\n\nGame URL: {}\nCreator: {}\nBet Size: {}\nMin Players: {}\nGrid Size: {}x{}\nBombs: {}",
            game_url, name, single_bet_size, min_players, grid, grid, bombs
        );
            if let Err(e) = send_telegram_message(&notification_message).await {
                error!("Failed to send Telegram notification: {}", e);
            }
        }

        // Register the new game session
        let session = GameSession {
            game_id: game_id.clone(),
            server_id: self.server_id.clone(),
            single_bet_size,
            min_players,
            current_players: 1,
            grid_size: grid,
        };
        self.discovery.register_game_session(session).await?;

        info!("Storing game state in local state");
        info!("--------------------------------");
        info!("Game state: {:?}", game_state);
        info!("--------------------------------");
        // Store in local state
        let mut games_write = self.games.write().await;
        games_write.insert(game_id.clone(), game_state.clone());

        Ok(Some(game_state))
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
        let server_id = env::var("FLY_MACHINE_ID").unwrap_or_else(|_| "LocalServer".to_string());

        Self {
            server_id: server_id.clone(),
            registry: GameRegistry::new(redis_client, server_id),
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
        mut stream: TcpStream,
    ) -> anyhow::Result<()> {
        // Read the HTTP request to check for cookies before accepting WebSocket connection
        let mut buf = [0; 8192];
        let n = stream.peek(&mut buf).await?;
        let data = &buf[..n];

        // Extract machine ID and handle redirection
        if let Some(target_machine_id) = extract_machine_id(&data, &server_id) {
            info!(
                "Redirecting WebSocket connection to machine: {}",
                target_machine_id
            );

            // Complete HTTP response with proper headers
            let response = format!(
                "HTTP/1.1 307 Temporary Redirect\r\n\
             fly-replay: instance={}\r\n\
             Content-Length: 0\r\n\
             Connection: close\r\n\r\n",
                target_machine_id
            );

            match stream.write_all(response.as_bytes()).await {
                Ok(_) => {
                    info!("Sent redirect response successfully");
                    // Make sure to flush the stream
                    if let Err(e) = stream.flush().await {
                        error!("Error flushing redirect response: {}", e);
                    }
                    return Ok(());
                }
                Err(e) => {
                    error!("Error sending redirect response: {}", e);
                    return Err(anyhow::anyhow!("Failed to send redirect response: {}", e));
                }
            }
        }
        let ws_stream = ServerBuilder::new().accept(stream).await?;
        let pool = establish_connection().await;

        let (ws_write, mut ws_read) = ws_stream.split();

        let ws_write = Arc::new(Mutex::new(ws_write));

        // Create a channel for this game connection
        let (server_tx, mut server_rx) = tokio::sync::mpsc::channel(500);
        let server_tx = Arc::new(server_tx);

        // Keep track of the current player_id for cleanup
        let current_player_id = Arc::new(RwLock::new(String::new()));

        // Spawn a task to handle incoming WebSocket messages
        let _ = tokio::spawn({
            let server_tx = server_tx.clone();
            let current_player_id = current_player_id.clone();
            let registry_clone = registry.clone();
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
                    let server_tx_inner = server_tx.clone();
                    let active_players_read = registry_clone.active_players.read().await;
                    let game_id = active_players_read.get(&player_id);
                    if let Some(game_id) = game_id {
                        let game_state = registry_clone.get_game_state(&game_id).await;
                        if let Some(GameState::RUNNING {
                            game_id,
                            players,
                            board,
                            single_bet_size,
                            ..
                        }) = game_state
                        {
                            let loser_idx = players.iter().position(|p| p.id == player_id).unwrap();
                            let new_game_state = GameState::FINISHED {
                                game_id,
                                loser_idx,
                                board,
                                players,
                                single_bet_size,
                            };

                            let game_message = GameMessage::GameUpdate(new_game_state);

                            server_tx_inner.send(game_message).await.unwrap();
                        }
                    }
                    drop(active_players_read);
                    info!("Cleaning up player: {}", player_id);
                    registry_clone.cleanup_player(&player_id).await;
                }
            }
        });
        // Process game messages
        while let Some(message) = server_rx.recv().await {
            match message {
                GameMessage::Ping { game_id, player_id } => {
                    info!("Pong sent from {}", server_id);
                    info!("Pong set from {}", server_id);
                    if let Some(game_id) = &game_id {
                        registry
                            .subscribe_to_channel(
                                server_id.clone(),
                                game_id.clone(),
                                ws_write.clone(),
                            )
                            .await?;
                    }

                    if let Some(player_id) = player_id {
                        let mut active_players_write = registry.active_players.write().await;
                        active_players_write.insert(player_id, game_id.unwrap());
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
                    name,
                    single_bet_size,
                    min_players,
                    bombs,
                    grid,
                } => {
                    info!("Play request at machine: {}", server_id);
                    let active_players_read = registry.active_players.read().await;

                    if active_players_read.contains_key(&player_id) {
                        info!("Player is already waiting for a game");
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

                    // Try to find or create a game using discovery service
                    match registry
                        .handle_play_message(
                            player_id.clone(),
                            name.clone(),
                            single_bet_size,
                            min_players,
                            bombs,
                            grid,
                        )
                        .await
                    {
                        Ok(Some(game_state)) => {
                            info!("created or joined on this server");
                            // Game was created or joined on this server
                            let game_id = match &game_state {
                                GameState::WAITING { game_id, .. } => game_id.clone(),
                                GameState::RUNNING { game_id, .. } => game_id.clone(),
                                _ => unreachable!(),
                            };

                            // Subscribe to game updates
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

                            let game_message = GameMessage::GameUpdate(game_state.clone());
                            info!("Game Message: {:?}", game_message);
                            let wrapper = GameMessageWrapper {
                                server_id: server_id.clone(),
                                game_message: game_message,
                            };

                            registry
                                .publish_message(game_id.clone(), wrapper, false)
                                .await?;

                            let mut active_players_write = registry.active_players.write().await;
                            active_players_write.insert(player_id, game_id);
                        }
                        Ok(None) => {
                            // Game exists on another server, send redirect message
                            if let Some(session) = registry
                                .discovery
                                .find_game_session(single_bet_size, min_players, grid)
                                .await?
                            {
                                let redirect = GameMessage::RedirectToServer {
                                    game_id: session.game_id,
                                    machine_id: session.server_id,
                                };
                                info!("--------------------------------");
                                info!("Redirecting to server: {:?}", redirect);
                                info!("--------------------------------");
                                ws_write
                                    .lock()
                                    .await
                                    .send(Message::binary(serde_json::to_vec(&redirect)?))
                                    .await?;
                            } else {
                                let response =
                                    GameMessage::Error("No suitable game found".to_string());
                                ws_write
                                    .lock()
                                    .await
                                    .send(Message::binary(serde_json::to_vec(&response)?))
                                    .await?;
                            }
                        }
                        Err(e) => {
                            let response =
                                GameMessage::Error(format!("Error handling play request: {}", e));
                            ws_write
                                .lock()
                                .await
                                .send(Message::binary(serde_json::to_vec(&response)?))
                                .await?;
                        }
                    }
                }
                GameMessage::Join {
                    game_id,
                    player_id,
                    name,
                } => {
                    info!("Join request at machine: {}", server_id);
                    info!("Request to join:: {:?} game", game_id);

                    // let games_read = registry.games.read().await;
                    // info!("Game keys: {:?}", games_read.keys().len());
                    let game_state = registry.get_game_state(&game_id).await;
                    // let game_state = registry.get_game_state(&game_id).await;
                    info!("Game state: {:?}", game_state);
                    info!("About to join game");
                    if let Some(GameState::WAITING {
                        game_id,
                        creator,
                        board,
                        single_bet_size,
                        min_players,
                        players,
                    }) = game_state
                    {
                        info!("Inside waiting state");
                        let new_player = Player::new(player_id.clone(), name.clone());
                        let mut players = players.clone();
                        players.push(new_player);

                        // Update player count in Redis
                        info!("Updating player count in Redis");
                        registry
                            .discovery
                            .update_player_count(&game_id, players.len() as u32)
                            .await?;

                        let new_game_state = if players.len() < min_players as usize {
                            GameState::WAITING {
                                game_id: game_id.clone(),
                                creator: creator.clone(),
                                board: board.clone(),
                                single_bet_size: single_bet_size,
                                min_players: min_players,
                                players,
                            }
                        } else {
                            // Game is transitioning to RUNNING state
                            // Remove from discovery since it's no longer accepting players
                            registry.discovery.remove_game_session(&game_id).await?;

                            GameState::RUNNING {
                                game_id: game_id.clone(),
                                players,
                                board: board.clone(),
                                turn_idx: 0,
                                single_bet_size: single_bet_size,
                                locks: None,
                            }
                        };

                        let mut games_write = registry.games.write().await;

                        games_write.insert(game_id.clone(), new_game_state.clone());

                        drop(games_write);

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
                        info!("Publishing message to game");
                        registry
                            .publish_message(game_id.clone(), wrapper, false)
                            .await?;
                        let mut active_players_write = registry.active_players.write().await;
                        active_players_write.insert(player_id, game_id);
                        info!("Player added to active players");
                    } else {
                        let game_session =
                            match registry.discovery.find_game_session_by_id(&game_id).await {
                                Ok(session) => session,
                                Err(_) => None,
                            };
                        if let Some(game_session) = game_session {
                            let redirect = GameMessage::RedirectToServer {
                                game_id: game_session.game_id,
                                machine_id: game_session.server_id,
                            };
                            info!("Redirecting to server: {:?}", redirect);
                            if let Err(err) = ws_write
                                .lock()
                                .await
                                .send(Message::binary(serde_json::to_vec(&redirect)?))
                                .await
                            {
                                eprintln!("Failed to send error message to the client:: {:?}", err);
                            }
                        } else {
                            info!("Game is not accepting players");
                            let response = GameMessage::Error(
                                "this game is not accepting players".to_string(),
                            );
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
                }
                GameMessage::Stop { game_id, abort } => {
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

                                active_players_write.retain(|x, _| !ids.contains(x));

                                // Update discovery service
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
                    } else {
                        // Game is being aborted
                        if let Some(game_state) = games_write.get_mut(&game_id) {
                            // if let GameState::RUNNING { players, .. } = game_state {
                            //     // remove players from active state
                            //     let mut active_players_write =
                            //         registry.active_players.write().await;
                            //     let ids = players.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
                            //     active_players_write.retain(|x, _| !ids.contains(x));
                            // }
                            match game_state {
                                GameState::RUNNING { players, .. } => {
                                    let mut active_players_write =
                                        registry.active_players.write().await;
                                    let ids =
                                        players.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
                                    active_players_write.retain(|x, _| !ids.contains(x));
                                }
                                GameState::WAITING { players, .. } => {
                                    let mut active_players_write =
                                        registry.active_players.write().await;
                                    let ids =
                                        players.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
                                    active_players_write.retain(|x, _| !ids.contains(x));
                                }
                                _ => {
                                    // Do nothing
                                }
                            }

                            let aborted_state = GameState::ABORTED {
                                game_id: game_id.clone(),
                            };
                            *game_state = aborted_state.clone();

                            // Update discovery service
                            registry
                                .save_game_state(game_id.clone(), aborted_state)
                                .await;

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
                }
                GameMessage::MakeMove { game_id, x, y } => {
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

                                    active_players_write.retain(|x, _| !ids.contains(x));

                                    // Update discovery service
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
                                    info!("Setting locks to None, befor locks value: {:?}", *locks);
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
                            turn_idx, players, ..
                        } = game_state
                        {
                            *turn_idx = (*turn_idx + 1) % players.len();
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

                GameMessage::RematchRequest { game_id, requester } => {
                    info!("--------------------------------");
                    info!("Rematch request received");
                    info!("--------------------------------");
                    let mut games_write = registry.games.write().await;
                    if let Some(game_state) = games_write.get_mut(&game_id) {
                        if let GameState::FINISHED {
                            game_id,
                            board,
                            players,
                            single_bet_size,
                            ..
                        } = game_state
                        {
                            let grid = board.n;
                            let bombs = board.bomb_coordinates.len();
                            let new_board = Board::new(grid as usize, bombs as usize);

                            let (index, player) = players
                                .iter()
                                .enumerate()
                                .find(|(idx, p)| *p.id == requester)
                                .expect("Failed to find player id in player array");

                            let mut rematch_acceptants = vec![0 as usize; players.len()];
                            rematch_acceptants[index] = 1;
                            let new_game_state = GameState::REMATCH {
                                game_id: game_id.clone(),
                                players: players.clone(),
                                board: new_board,
                                single_bet_size: single_bet_size.clone(),
                                accepted: rematch_acceptants,
                            };

                            let game_message = GameMessage::RematchRequest {
                                game_id: game_id.clone(),
                                requester: requester.clone(),
                            };

                            let wrapper = GameMessageWrapper {
                                server_id: server_id.clone(),
                                game_message,
                            };

                            registry
                                .publish_message(game_id.clone(), wrapper.clone(), false)
                                .await?;

                            *game_state = new_game_state.clone();
                        }
                    }
                }

                GameMessage::RematchResponse {
                    game_id,
                    player_id,
                    want_rematch,
                } => {
                    let mut games_write = registry.games.write().await;
                    if let Some(game_state) = games_write.get_mut(&game_id) {
                        if let GameState::REMATCH {
                            game_id,
                            players,
                            board,
                            single_bet_size,
                            accepted,
                        } = game_state
                        {
                            if want_rematch {
                                let (index, player) = players
                                    .iter()
                                    .enumerate()
                                    .find(|(_, p)| *p.id == player_id)
                                    .expect("Failed to find player id in player array");

                                accepted[index] = 1;

                                if accepted.iter().all(|&x| x == 1) {
                                    let new_game_state = GameState::RUNNING {
                                        game_id: game_id.clone(),
                                        players: players.clone(),
                                        board: board.clone(),
                                        turn_idx: 0,
                                        single_bet_size: single_bet_size.clone(),
                                        locks: None,
                                    };

                                    let game_message =
                                        GameMessage::GameUpdate(new_game_state.clone());
                                    let wrapper = GameMessageWrapper {
                                        server_id: server_id.clone(),
                                        game_message,
                                    };

                                    registry
                                        .publish_message(game_id.clone(), wrapper.clone(), false)
                                        .await?;
                                    *game_state = new_game_state.clone();
                                }
                            } else {
                                let new_game_state = GameState::ABORTED {
                                    game_id: game_id.clone(),
                                };
                                let game_message = GameMessage::GameUpdate(new_game_state.clone());
                                let wrapper = GameMessageWrapper {
                                    server_id: server_id.clone(),
                                    game_message,
                                };

                                registry
                                    .publish_message(game_id.clone(), wrapper.clone(), false)
                                    .await?;
                            }
                        }
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

                            // / remove players from active state
                            let mut active_players_write = registry.active_players.write().await;

                            let ids = players.iter().map(|p| p.id.clone()).collect::<Vec<_>>();

                            active_players_write.retain(|x, _| !ids.contains(x));
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
                        _ => {}
                    }
                }
                GameMessage::RedirectToServer { .. } => {
                    unreachable!("Should fail if execution enters here");
                    // // Send the redirect message to the client
                    // let redirect = GameMessage::RedirectToServer {
                    //     game_id,
                    //     machine_id,
                    // };
                    // if let Err(e) = ws_write
                    //     .lock()
                    //     .await
                    //     .send(Message::binary(serde_json::to_vec(&redirect)?))
                    //     .await
                    // {
                    //     eprintln!("Error sending redirect message: {}", e);
                    // }
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

// Helper function to parse HTTP headers from a byte slice
fn parse_http_headers(data: &[u8]) -> Result<HashMap<String, HeaderValue>, anyhow::Error> {
    let mut headers = HashMap::new();

    if let Ok(request_str) = std::str::from_utf8(data) {
        // Split the request into lines
        let lines: Vec<&str> = request_str.split("\r\n").collect();

        // Skip the request line and parse headers
        for line in lines.iter().skip(1) {
            if line.is_empty() {
                break; // End of headers
            }

            if let Some(idx) = line.find(':') {
                let key = line[..idx].trim().to_lowercase();
                let value = line[idx + 1..].trim();

                if let Ok(header_value) = HeaderValue::from_str(value) {
                    headers.insert(key, header_value);
                }
            }
        }
    }

    Ok(headers)
}

// Helper function to parse cookies from a header value
fn parse_cookies(cookie_header: Option<&HeaderValue>) -> HashMap<String, String> {
    let mut cookies = HashMap::new();

    if let Some(header) = cookie_header {
        if let Ok(cookie_str) = header.to_str() {
            for cookie_pair in cookie_str.split(';') {
                let mut parts = cookie_pair.trim().splitn(2, '=');
                if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                    cookies.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }
    }

    cookies
}

// Function to parse the HTTP request URI from raw bytes
fn parse_request_uri(data: &[u8]) -> Option<String> {
    if let Ok(request_str) = std::str::from_utf8(data) {
        // HTTP request first line format: "GET /path?query HTTP/1.1"
        let first_line = request_str.lines().next()?;
        let parts: Vec<&str> = first_line.split_whitespace().collect();

        if parts.len() >= 2 {
            return Some(parts[1].to_string());
        }
    }
    None
}

// Parse query parameters from a URI string
fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();

    for param_pair in query.split('&') {
        let mut pair = param_pair.split('=');
        if let (Some(key), Some(value)) = (pair.next(), pair.next()) {
            // URL decode the key and value
            if let (Ok(decoded_key), Ok(decoded_value)) =
                (urlencoding::decode(key), urlencoding::decode(value))
            {
                params.insert(decoded_key.into_owned(), decoded_value.into_owned());
            } else {
                // Fall back to raw values if decoding fails
                params.insert(key.to_string(), value.to_string());
            }
        }
    }

    params
}

// Extract the machine ID from a WebSocket request
fn extract_machine_id(data: &[u8], server_id: &str) -> Option<String> {
    info!("Extracting machine ID");
    // Try to get machine ID from URL parameter
    if let Some(uri) = parse_request_uri(data) {
        info!("URI: {}", uri);
        if let Some(query_pos) = uri.find('?') {
            let query = &uri[query_pos + 1..];
            let params = parse_query_string(query);

            if let Some(machine_id) = params.get("machine_id") {
                // If request targets a different machine, return it
                if machine_id != server_id {
                    println!("Machine ID: {}", machine_id);
                    return Some(machine_id.clone());
                }
            }
        }
    }

    // Try to get machine ID from cookies as fallback
    if let Ok(headers) = parse_http_headers(data) {
        let cookies = parse_cookies(headers.get("cookie"));
        if let Some(machine_id) = cookies.get("fly-machine-id") {
            if machine_id != server_id {
                return Some(machine_id.clone());
            }
        }
    }

    None
}
