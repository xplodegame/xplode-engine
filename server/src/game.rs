use common::db::{self, establish_connection};
use futures_util::{
    lock::Mutex,
    stream::{SplitSink, StreamExt},
    SinkExt,
};

use redis::{AsyncCommands, Client};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        broadcast::{self},
        mpsc, RwLock,
    },
};
use tokio_websockets::{Message, ServerBuilder, WebSocketStream};

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
    Stop {
        game_id: String,
        abort: bool,
    },
    Ping {
        game_id: Option<String>,
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
    pub async fn save_game_state(&self, game_id: String, state: GameState) {
        // Sync to Redis (async write to avoid blocking)
        let mut conn = self.redis.get_multiplexed_async_connection().await.unwrap();
        conn.set_ex::<String, String, ()>(game_id, serde_json::to_string(&state).unwrap(), 300)
            .await
            .expect("Faield to update game state");
    }

    pub async fn load_game_state(&self) {
        let mut conn = self.redis.get_multiplexed_async_connection().await.unwrap();

        let keys: Vec<String> = conn.keys("*").await.unwrap();
        println!(
            "***************************Keys length: {}************************************",
            keys.len()
        );

        for key in keys {
            if let Ok(state) = conn.get::<_, String>(key.clone()).await {
                if let Ok(game_state) = serde_json::from_str::<GameState>(&state) {
                    self.games.write().await.insert(key, game_state);
                }
            }
        }
    }

    // Load Game State from Redis if Not in Memory
    pub async fn get_game_state(&self, game_id: &str) -> Option<GameState> {
        let mut game_states = self.games.write().await;

        if let Some(state) = game_states.get(game_id) {
            return Some(state.clone());
        }

        // Not in-memory: Load from Redis
        let mut conn = self.redis.get_multiplexed_async_connection().await.unwrap();
        let redis_value: Option<String> = redis::cmd("GET")
            .arg(game_id)
            .query_async(&mut conn)
            .await
            .ok();

        if let Some(state_json) = redis_value {
            if let Ok(game_state) = serde_json::from_str::<GameState>(&state_json) {
                game_states.insert(game_id.to_string(), game_state.clone());
                return Some(game_state);
            }
        }
        None
    }

    // Load Game State from Redis if Not in Memory
    pub async fn load_game_if_absent(&self, game_id: &str) {
        let mut game_states = self.games.write().await;

        if let Some(_) = game_states.get(game_id) {
            return;
        }

        // Not in-memory: Load from Redis
        let mut conn = self.redis.get_multiplexed_async_connection().await.unwrap();
        let redis_value: Option<String> = redis::cmd("GET")
            .arg(game_id)
            .query_async(&mut conn)
            .await
            .ok();

        if let Some(state_json) = redis_value {
            if let Ok(game_state) = serde_json::from_str::<GameState>(&state_json) {
                game_states.insert(game_id.to_string(), game_state.clone());
                return;
            }
        }
    }

    pub async fn subscribe_to_channel(
        &self,
        server_id: String,
        channel: String,
        ws_write: Arc<Mutex<WebSocketSink>>,
    ) {
        println!("SUbscribed to channel: {:?}", channel);
        // Channel name should only rely on game_id
        let mut broadcast_channels = self.broadcast_channels.write().await;

        if broadcast_channels.get(&channel).is_none() {
            let (tx, _rx) = broadcast::channel(100);
            broadcast_channels.insert(channel.clone(), tx.clone());
            println!("Inserting into broadcast channel");

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
                                    .await;
                            }
                        }
                    }
                }
            });
        }
        println!("################Broadcasting now\n");
        let broadcast_tx = broadcast_channels.get(&channel).unwrap();
        let mut broadcast_rx = broadcast_tx.subscribe();

        println!(
            "broadcast_tx.receiver_count(): {}",
            broadcast_tx.receiver_count()
        );
        tokio::spawn(async move {
            while let Ok(game_message) = broadcast_rx.recv().await {
                println!("Got one message");
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
    }

    // ✅ Publish messages using multiplexed connection
    pub async fn publish_message(
        &self,
        channel: String, // channel == game_id
        game_message_wrapper: GameMessageWrapper,
        from_redis: bool,
    ) {
        println!("***********publishing message***********");
        // Send to local clients
        if let Some(channel) = self.broadcast_channels.read().await.get(&channel) {
            let _ = channel.send(game_message_wrapper.game_message.clone());
        }

        // Publish to Redis only if not from Redis
        if !from_redis {
            let mut conn = self.redis.get_multiplexed_async_connection().await.unwrap();
            let _ = conn
                .publish::<String, String, ()>(
                    channel,
                    serde_json::to_string(&game_message_wrapper).unwrap(),
                )
                .await;
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
        let redis_client = Client::open(redis_url).unwrap();

        Self {
            server_id: Uuid::new_v4().to_string(),
            registry: GameRegistry {
                games: Arc::new(RwLock::new(HashMap::new())),
                game_channels: Arc::new(RwLock::new(HashMap::new())),
                broadcast_channels: Arc::new(RwLock::new(HashMap::new())),
                // player_streams: Arc::new(RwLock::new(HashMap::new())),
                redis: redis_client,
            },
        }
    }

    pub async fn start(&self, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        println!("Server listening on {}", addr);

        // let (tx, _rx) = broadcast::channel::<Message>(100);

        while let std::result::Result::Ok((stream, _)) = listener.accept().await {
            let registry = self.registry.clone();
            let server_id = self.server_id.clone();

            // let tx = tx.clone();
            tokio::spawn(async move {
                println!("Establishing connection");
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
        // ✅ Each client gets its own Receiver
        // let mut broadcast_rx = broadcast_tx.subscribe();

        let (ws_write, mut ws_read) = ws_stream.split();

        let ws_write = Arc::new(Mutex::new(ws_write));

        // Create a channel for this game connection
        let (server_tx, mut server_rx) = tokio::sync::mpsc::channel(500);
        let server_tx = Arc::new(server_tx);

        // Spawn a task to handle incoming WebSocket messages
        tokio::spawn({
            let server_tx = server_tx.clone();
            async move {
                while let Some(msg) = ws_read.next().await {
                    println!("Incoming msg");
                    let server_tx_inner = server_tx.clone();

                    match msg {
                        Ok(message) => {
                            tokio::spawn(async move {
                                match serde_json::from_slice(message.as_payload()) {
                                    Ok(game_msg) => {
                                        println!("msg: {:?}", game_msg);
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
                        }
                    }
                }
            }
        });

        // let ws_write_clone = ws_write.clone();
        // let writers_task = tokio::spawn(async move {
        //     while let Ok(msg) = broadcast_rx.recv().await {
        //         let mut sink = ws_write_clone.lock().await;
        //         if sink.send(msg.into()).await.is_err() {
        //             // FIXME: Player id to print
        //             eprintln!("Player disconnected");
        //             break;
        //         }
        //     }
        // });

        // Process game messages
        while let Some(message) = server_rx.recv().await {
            match message {
                GameMessage::Ping { game_id } => {
                    if let Some(game_id) = game_id {
                        registry
                            .subscribe_to_channel(server_id.clone(), game_id, ws_write.clone())
                            .await;
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
                    println!("Play request");
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
                        println!("Loading game from memory");
                        // try to match game from redis

                        registry.load_game_state().await;

                        // FIXME: Remove redundant checking
                        let games_read = registry.games.read().await;

                        println!("Keys length after: {:?}", games_read.keys().len());
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
                        println!("Game has begun");
                        let player = Player::new(player_id);
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
                            .await;
                        // let mut player_streams_write = registry.player_streams.write().await;
                        // player_streams_write
                        //     .entry(game_id.clone())
                        //     .or_insert_with(Vec::new)
                        //     .push(ws_write.clone());

                        let mut game_channels_write = registry.game_channels.write().await;
                        game_channels_write.insert(game_id.clone(), server_tx.clone());
                        drop(game_channels_write);

                        // Get the channel for this game
                        println!("Getting the channel for this game????");
                        let game_channels_read = registry.game_channels.read().await;
                        if let Some(channel) = game_channels_read.get(&game_id) {
                            println!("Sending a message to all the players");
                            // Broadcast game state to all players
                            let response = GameMessage::GameUpdate(new_game_state.clone());
                            channel.send(response).await?;
                        }
                    } else {
                        println!("User will create a game");
                        let game_id = Uuid::new_v4().to_string();
                        let board = Board::new(grid as usize, bombs as usize);
                        let player = Player::new(player_id);

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
                            .await;
                        // let mut player_streams_write = registry.player_streams.write().await;
                        // player_streams_write
                        //     .entry(game_id.clone())
                        //     .or_insert_with(Vec::new)
                        //     .push(ws_write.clone());
                        // registry.subscribe_to_channel(server_id, game_id, ws_write.clone());

                        // let wrapper = GameMessageWrapper {
                        //     server_id,
                        //     game_message: response,
                        // };

                        // registry
                        //     .publish_message(game_id.clone(), wrapper, false)
                        //     .await;

                        // FIXME: Better solution required
                        let response = GameMessage::GameUpdate(game_state);
                        let game_channel_read = registry.game_channels.read().await;
                        if let Some(channel) = game_channel_read.get(&game_id) {
                            channel.send(response.clone()).await?;
                        }
                        if let Err(e) = ws_write
                            .lock()
                            .await
                            .send(Message::binary(serde_json::to_vec(&response)?))
                            .await
                        {
                            eprintln!("Error sending GameUpdate message: {}", e);
                        }
                    }
                }
                GameMessage::Join { game_id, player_id } => {
                    println!("Request to join:: {:?} game", game_id);

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
                        let new_player = Player::new(player_id);
                        let mut players = players.clone();
                        players.push(new_player);

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
                            GameState::RUNNING {
                                game_id: game_id.clone(),
                                players: players,
                                board: board.clone(),
                                turn_idx: 0,
                                single_bet_size: single_bet_size,
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
                            .await;

                        // let wrapper = GameMessageWrapper {
                        //     server_id,
                        //     game_message: GameMessage::GameUpdate(new_game_state.clone()),
                        // };

                        // registry
                        //     .publish_message(game_id.clone(), wrapper, false)
                        //     .await;

                        // let mut player_streams_write = registry.player_streams.write().await;
                        // player_streams_write
                        //     .entry(game_id.clone())
                        //     .or_insert_with(Vec::new)
                        //     .push(ws_write.clone());

                        // Get the channel for this game
                        let game_channels_read = registry.game_channels.read().await;
                        if let Some(channel) = game_channels_read.get(&game_id) {
                            // Broadcast game state to all players
                            let response = GameMessage::GameUpdate(new_game_state.clone());
                            channel.send(response).await?;
                        }
                    } else {
                        let response =
                            GameMessage::Error("this game is not accepting players".to_string());
                        if let Err(err) = ws_write
                            .clone()
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
                                *game_state = GameState::FINISHED {
                                    game_id: game_id.clone(),
                                    loser_idx: (*turn_idx + 1) % 2,
                                    board: board.clone(),
                                    players: players.clone(),
                                    single_bet_size: single_bet_size.clone(),
                                };

                                // asyncrhonously save the state in redis
                                registry
                                    .save_game_state(game_id.clone(), game_state.clone())
                                    .await;

                                // Get the channel to broadcast game state
                                let game_channels_read = registry.game_channels.read().await;
                                if let Some(channel) = game_channels_read.get(&game_id) {
                                    let response = GameMessage::GameUpdate(game_state.clone());
                                    channel.send(response).await?;
                                }
                            }
                        }
                    } else {
                        if let Some(game_state) = games_write.get_mut(&game_id) {
                            *game_state = GameState::ABORTED {
                                game_id: game_id.clone(),
                            };

                            let game_channel_read = registry.game_channels.read().await;
                            let response = GameMessage::GameUpdate(game_state.clone());
                            if let Some(channel) = game_channel_read.get(&game_id) {
                                channel.send(response).await?;
                            }
                        }
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
                                ..
                            } => {
                                // TODO add backend logic to check turn
                                if board.mine(x, y) {
                                    // If mine is hit, determine winner
                                    let loser = turn_idx;
                                    *game_state = GameState::FINISHED {
                                        game_id: game_id.clone(),
                                        loser_idx: *loser,
                                        board: board.clone(),
                                        players: players.clone(),
                                        single_bet_size: single_bet_size.clone(),
                                    };

                                    // asyncrhonously save the state in redis
                                    registry
                                        .save_game_state(game_id.clone(), game_state.clone())
                                        .await;
                                } else {
                                    // Switch turns
                                    *turn_idx = (*turn_idx + 1) % players.len();
                                }

                                // Get the channel to broadcast game state
                                let game_channels_read = registry.game_channels.read().await;
                                if let Some(channel) = game_channels_read.get(&game_id) {
                                    let response = GameMessage::GameUpdate(game_state.clone());
                                    channel.send(response).await?;
                                }
                            }
                            _ => {
                                // FIXME: I think it is not relevant
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
                GameMessage::GameUpdate(msg) => {
                    let game_message = GameMessage::GameUpdate(msg.clone());

                    let wrapper = GameMessageWrapper {
                        server_id: server_id.clone(),
                        game_message,
                    };
                    println!("Inside game update");
                    match msg {
                        GameState::RUNNING { game_id, .. } => {
                            registry
                                .publish_message(game_id.clone(), wrapper, false)
                                .await;
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
                                .await;
                            // Update the db
                            let winning_amount = single_bet_size / ((players.len() - 1) as f64);

                            let user_ids: Vec<u32> = players
                                .iter()
                                .map(|p| p.id.parse::<u32>().unwrap())
                                .collect();
                            db::update_player_balances(
                                &pool,
                                &user_ids,
                                loser_idx,
                                single_bet_size,
                                winning_amount,
                            )
                            .await?;
                        }
                        GameState::ABORTED { game_id } => {
                            registry
                                .publish_message(game_id.clone(), wrapper, false)
                                .await;
                        }
                        GameState::WAITING { game_id, .. } => {
                            registry
                                .publish_message(game_id.clone(), wrapper, false)
                                .await;
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

// println!("Game update here");

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
