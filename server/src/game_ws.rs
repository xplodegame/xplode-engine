use futures_util::{
    lock::Mutex,
    stream::{SplitSink, StreamExt},
    SinkExt,
};

use http::Uri;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, RwLock},
};
use tokio_websockets::{ClientBuilder, MaybeTlsStream, Message, ServerBuilder, WebSocketStream};

use uuid::Uuid;

use crate::{board::Board, player::Player};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameState {
    WAITING {
        game_id: String,
        creator: Player,
        board: Board,
    },
    RUNNING {
        game_id: String,
        players: Vec<Player>,
        board: Board,
        turn_idx: usize,
    },
    FINISHED {
        game_id: String,
        winner_idx: usize,
        board: Board,
        players: Vec<Player>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameMessage {
    Play { player_id: String },
    MakeMove { game_id: String, x: usize, y: usize },
    GameUpdate(GameState),
    Error(String),
    Dummy,
}

pub struct GameServer {
    games: Arc<RwLock<HashMap<String, GameState>>>,
    game_channels: Arc<RwLock<HashMap<String, mpsc::Sender<GameMessage>>>>,
    player_streams: Arc<
        RwLock<HashMap<String, Vec<Arc<Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>>>>,
    >,
}

impl GameServer {
    pub fn new() -> Self {
        Self {
            games: Arc::new(RwLock::new(HashMap::new())),
            game_channels: Arc::new(RwLock::new(HashMap::new())),
            player_streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start(&self, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        println!("Server listening on {}", addr);

        while let std::result::Result::Ok((stream, _)) = listener.accept().await {
            let handler = GameConnectionHandler::new(
                self.games.clone(),
                self.game_channels.clone(),
                self.player_streams.clone(),
            );

            tokio::spawn(async move {
                if let Err(e) = handler.handle_connection(stream).await {
                    eprintln!("Error handling connection: {}", e);
                }
            });
        }

        Ok(())
    }
}

pub struct GameConnectionHandler {
    games: Arc<RwLock<HashMap<String, GameState>>>,
    game_channels: Arc<RwLock<HashMap<String, mpsc::Sender<GameMessage>>>>,
    player_streams: Arc<
        RwLock<HashMap<String, Vec<Arc<Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>>>>,
    >,
}

impl GameConnectionHandler {
    pub fn new(
        games: Arc<RwLock<HashMap<String, GameState>>>,
        game_channels: Arc<RwLock<HashMap<String, mpsc::Sender<GameMessage>>>>,
        player_streams: Arc<
            RwLock<
                HashMap<String, Vec<Arc<Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>>>,
            >,
        >,
    ) -> Self {
        Self {
            games,
            game_channels,
            player_streams,
        }
    }

    async fn handle_connection(&self, stream: TcpStream) -> anyhow::Result<()> {
        let ws_stream = ServerBuilder::new().accept(stream).await?;

        let (ws_write, mut ws_read) = ws_stream.split();

        let ws_write = Arc::new(Mutex::new(ws_write));

        // Create a channel for this game connection
        let (tx, mut rx) = mpsc::channel(100);

        // Spawn a task to handle incoming WebSocket messages
        let handlers = tokio::spawn({
            let tx_clone = tx.clone();
            async move {
                while let Some(msg) = ws_read.next().await {
                    match msg {
                        Ok(message) => match serde_json::from_slice(message.as_payload()) {
                            Ok(game_msg) => {
                                println!("msg: {:?}", game_msg);
                                if let Err(e) = tx_clone.send(game_msg).await {
                                    eprintln!("Error sending message: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                eprintln!("Deserialization error: {}", e);
                            }
                        },
                        Err(e) => {
                            eprintln!("WebSocket error: {}", e);
                            break;
                        }
                    }
                }
            }
        });

        // Process game messages
        while let Some(message) = rx.recv().await {
            // testing

            println!("Game Message: {:?}", message);
            match message {
                GameMessage::Play { player_id } => {
                    println!("Hello Play message received");
                    let games_read = self.games.read().await;

                    // Check for a waiting game
                    if let Some((game_id, GameState::WAITING { creator, board, .. })) = games_read
                        .iter()
                        .find(|(_, state)| matches!(state, GameState::WAITING { .. }))
                        .map(|(game_id, state)| (game_id.clone(), state.clone()))
                    {
                        drop(games_read);
                        // Now we can safely create a new player and prepare the new game state
                        println!("User will join the game");
                        let player = Player::new(player_id);
                        let players = vec![creator.clone(), player.clone()];

                        let new_game_state = GameState::RUNNING {
                            game_id: game_id.clone(),
                            players,
                            board: board.clone(),
                            turn_idx: 0,
                        };

                        {
                            let mut games_write = self.games.write().await;
                            games_write.insert(game_id.clone(), new_game_state.clone());
                        }
                        let mut player_streams_write = self.player_streams.write().await;
                        player_streams_write
                            .entry(game_id.clone())
                            .or_insert_with(Vec::new)
                            .push(ws_write.clone());

                        // Get the channel for this game
                        let game_channels_read = self.game_channels.read().await;
                        if let Some(channel) = game_channels_read.get(&game_id) {
                            // Broadcast game state to all players
                            let response = GameMessage::GameUpdate(new_game_state.clone());
                            channel.send(response.clone()).await?;
                        }
                    } else {
                        drop(games_read);
                        println!("User will create a game");
                        let game_id = Uuid::new_v4().to_string();
                        let board = Board::new(5);
                        let player = Player::new(player_id);

                        let game_state = GameState::WAITING {
                            game_id: game_id.clone(),
                            creator: player,
                            board,
                        };

                        println!("Taking the write lock");

                        // Store game state and channel
                        {
                            let mut games_write = self.games.write().await;
                            games_write.insert(game_id.clone(), game_state.clone());
                        }
                        let mut game_channels_write = self.game_channels.write().await;
                        game_channels_write.insert(game_id.clone(), tx.clone());

                        let mut player_streams_write = self.player_streams.write().await;
                        player_streams_write
                            .entry(game_id.clone())
                            .or_insert_with(Vec::new)
                            .push(ws_write.clone());

                        println!("Sending message to client");
                        // Send back the game state
                        let response = GameMessage::GameUpdate(game_state);
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
                GameMessage::MakeMove { game_id, x, y } => {
                    let mut games_write = self.games.write().await;

                    if let Some(game_state) = games_write.get_mut(&game_id) {
                        match game_state {
                            GameState::RUNNING {
                                players,
                                board,
                                turn_idx,
                                ..
                            } => {
                                // Check if it's the correct player's turn
                                // if players[*turn_idx].id == process::id().to_string() {
                                if board.mine(x, y) {
                                    // If mine is hit, determine winner
                                    let winner = (*turn_idx + 1) % 2;
                                    *game_state = GameState::FINISHED {
                                        game_id: game_id.clone(),
                                        winner_idx: winner,
                                        board: board.clone(),
                                        players: players.clone(),
                                    };
                                } else {
                                    // Switch turns
                                    *turn_idx = (*turn_idx + 1) % 2;
                                }

                                // Get the channel to broadcast game state
                                let game_channels_read = self.game_channels.read().await;
                                if let Some(channel) = game_channels_read.get(&game_id) {
                                    let response = GameMessage::GameUpdate(game_state.clone());
                                    channel.send(response).await?;
                                }
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
                GameMessage::GameUpdate(msg) => match msg {
                    GameState::RUNNING {
                        game_id,
                        players,
                        board,
                        turn_idx,
                    } => {
                        let player_streams =
                            self.player_streams.read().await.get(&game_id).cloned();

                        if let Some(player_streams) = player_streams {
                            let response = GameMessage::GameUpdate(GameState::RUNNING {
                                game_id,
                                players,
                                board,
                                turn_idx,
                            });

                            player_streams.iter().for_each(|stream| {
                                let response = response.clone();
                                let stream = stream.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = stream
                                        .lock()
                                        .await
                                        .send(Message::binary(
                                            serde_json::to_vec(&response).unwrap(),
                                        ))
                                        .await
                                    {
                                        eprintln!("Error sending message: {}", e);
                                    }
                                });
                            });
                        }
                    }
                    GameState::FINISHED {
                        game_id,
                        winner_idx,
                        board,
                        players,
                    } => {
                        let player_streams =
                            self.player_streams.read().await.get(&game_id).cloned();

                        if let Some(player_streams) = player_streams {
                            let response = GameMessage::GameUpdate(GameState::FINISHED {
                                game_id,
                                winner_idx,
                                board,
                                players,
                            });

                            player_streams.iter().for_each(|stream| {
                                let response = response.clone();
                                let stream = stream.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = stream
                                        .lock()
                                        .await
                                        .send(Message::binary(
                                            serde_json::to_vec(&response).unwrap(),
                                        ))
                                        .await
                                    {
                                        eprintln!("Error sending message: {}", e);
                                    }
                                });
                            });
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        handlers.await?;
        Ok(())
    }
}

// Client-side example (pseudo-code)
pub struct GameClient {
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,

    pub player_id: String,
}

impl GameClient {
    pub async fn new(url: &str) -> anyhow::Result<GameClient> {
        let uri = Uri::try_from(url)?;

        let (client, _) = ClientBuilder::from_uri(uri).connect().await?;
        Ok(GameClient {
            ws_stream: client,
            player_id: Uuid::new_v4().to_string(),
        })
    }

    pub async fn run_client(self) -> anyhow::Result<()> {
        // Split the WebSocket stream first
        let (mut ws_write, mut ws_read) = self.ws_stream.split();
        let (tx, mut rx) = mpsc::channel(100);

        // Spawn message handler task first to ensure we don't miss any messages
        let msg_handler = tokio::spawn({
            let tx_clone = tx.clone();
            async move {
                while let Some(msg) = ws_read.next().await {
                    match msg {
                        Ok(message) => {
                            println!("Received message from server");
                            match serde_json::from_slice(message.as_payload()) {
                                Ok(response) => {
                                    if let Err(e) = tx_clone.send(response).await {
                                        eprintln!("Error sending to channel: {}", e);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Error deserializing message: {}", e);
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("WebSocket error: {}", e);
                            break;
                        }
                    }
                }
                println!("Message handler loop ended");
                anyhow::Ok(())
            }
        });

        // Now that the message handler is set up, send the initial game message

        let play_msg = GameMessage::Play {
            player_id: self.player_id.clone(),
        };
        ws_write
            .send(Message::binary(serde_json::to_vec(&play_msg)?))
            .await?;
        println!("GameMessage::Play message sent");

        // Main game loop
        tokio::select! {
            result = msg_handler => {
                if let Err(e) = result {
                    eprintln!("Message handler error: {}", e);
                }
                println!("WebSocket connection closed");
            }
            result = async {
                while let Some(message) = rx.recv().await {
                    println!("Processing message: {:?}", message);
                    match &message {
                        GameMessage::GameUpdate(game_state) => {
                            match game_state {
                                GameState::RUNNING { players, board, turn_idx, game_id } => {
                                    println!("\nCurrent game state:");
                                    board.display();
                                    println!("Game ID: {}", game_id);
                                    if players[*turn_idx].id == self.player_id {
                                        println!("\nYour turn! Enter coordinates (x y):");
                                        let mut input = String::new();
                                        io::stdin().read_line(&mut input)?;
                                        let coords: Vec<&str> = input.trim().split_whitespace().collect();

                                        if coords.len() == 2 {
                                            if let (Ok(x), Ok(y)) = (coords[0].parse(), coords[1].parse()) {
                                                let move_msg = GameMessage::MakeMove {
                                                    game_id: game_id.clone(),
                                                    x,
                                                    y,
                                                };
                                                println!("Sending move: {:?}", move_msg);
                                                ws_write
                                                    .send(Message::binary(serde_json::to_vec(&move_msg)?))
                                                    .await?;
                                            } else {
                                                println!("Invalid coordinates! Please enter numbers.");
                                            }
                                        } else {
                                            println!("Invalid input! Please enter two numbers separated by space.");
                                        }
                                    } else {
                                        println!("\nWaiting for other player's move...");
                                    }
                                }
                                GameState::WAITING { game_id, .. } => {
                                    println!("Waiting for other player to join...");
                                    println!("Game ID: {}", game_id);
                                }
                                GameState::FINISHED { winner_idx, board, players, .. } => {
                                    println!("\nGame Over!");
                                    board.display();
                                    println!("Winner: Player {} ({})", winner_idx + 1, players[*winner_idx].id);
                                    return Ok(());
                                }
                            }
                        }
                        GameMessage::Error(err) => {
                            eprintln!("Game error: {}", err);
                        }
                        _ => {}
                    }
                }
                Ok::<(), anyhow::Error>(())
            } => {}
        }

        Ok(())
    }
}
