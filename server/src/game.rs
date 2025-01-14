use futures_util::{
    lock::Mutex,
    stream::{SplitSink, StreamExt},
    SinkExt,
};

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, RwLock},
};
use tokio_websockets::{Message, ServerBuilder, WebSocketStream};
use wallet::db::{self, establish_connection};

use uuid::Uuid;

use crate::{board::Board, player::Player};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameState {
    WAITING {
        game_id: String,
        creator: Player,
        board: Board,
        single_bet_size: u32,
    },
    RUNNING {
        game_id: String,
        players: Vec<Player>,
        board: Board,
        turn_idx: usize,
        single_bet_size: u32,
    },
    FINISHED {
        game_id: String,
        winner_idx: usize,
        board: Board,
        players: Vec<Player>,
        single_bet_size: u32,
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
        single_bet_size: u32,
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
    GameUpdate(GameState),
    Error(String),
}

#[derive(Clone)]
pub struct GamesShared {
    games: Arc<RwLock<HashMap<String, GameState>>>,
    game_channels: Arc<RwLock<HashMap<String, mpsc::Sender<GameMessage>>>>,
    player_streams: Arc<
        RwLock<HashMap<String, Vec<Arc<Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>>>>,
    >,
}

pub struct GameServer {
    games_shared: GamesShared,
}

impl GameServer {
    pub fn new() -> Self {
        Self {
            games_shared: GamesShared {
                games: Arc::new(RwLock::new(HashMap::new())),
                game_channels: Arc::new(RwLock::new(HashMap::new())),
                player_streams: Arc::new(RwLock::new(HashMap::new())),
            },
        }
    }

    pub async fn start(&self, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        println!("Server listening on {}", addr);

        while let std::result::Result::Ok((stream, _)) = listener.accept().await {
            let handler = GameConnectionHandler::new(self.games_shared.clone());

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
    games_shared: GamesShared,
}

impl GameConnectionHandler {
    pub fn new(games_shared: GamesShared) -> Self {
        Self { games_shared }
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
            match message {
                GameMessage::Play {
                    player_id,
                    single_bet_size,
                } => {
                    let games_read = self.games_shared.games.read().await;

                    let matched_game = games_read.iter().find_map(|(game_id, state)| {
                        if let GameState::WAITING {
                            single_bet_size: size,
                            ..
                        } = state
                        {
                            if *size == single_bet_size {
                                Some((game_id.clone(), state.clone()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });
                    if let Some((
                        game_id,
                        GameState::WAITING {
                            creator,
                            board,
                            single_bet_size,
                            ..
                        },
                    )) = matched_game
                    {
                        // Now we can safely create a new player and prepare the new game state
                        println!("Game has begun");
                        let player = Player::new(player_id);
                        let players = vec![creator.clone(), player.clone()];

                        let new_game_state = GameState::RUNNING {
                            game_id: game_id.clone(),
                            players: players.clone(),
                            board: board.clone(),
                            turn_idx: 0,
                            single_bet_size,
                        };

                        {
                            let mut games_write = self.games_shared.games.write().await;
                            games_write.insert(game_id.clone(), new_game_state.clone());
                        }
                        let mut player_streams_write =
                            self.games_shared.player_streams.write().await;
                        player_streams_write
                            .entry(game_id.clone())
                            .or_insert_with(Vec::new)
                            .push(ws_write.clone());

                        // Get the channel for this game
                        let game_channels_read = self.games_shared.game_channels.read().await;
                        if let Some(channel) = game_channels_read.get(&game_id) {
                            // Broadcast game state to all players
                            let response = GameMessage::GameUpdate(new_game_state.clone());
                            channel.send(response.clone()).await?;
                        }

                        let pool = establish_connection().await;
                        let player0_id: i32 = players[0].id.parse()?;
                        let player0 = db::get_user(&pool, player0_id).await?;

                        let player1_id: i32 = players[1].id.parse()?;
                        let player1 = db::get_user(&pool, player1_id).await?;

                        let player0_balance = player0.wallet_amount - single_bet_size as i32;
                        let player1_balance = player1.wallet_amount - single_bet_size as i32;
                        db::update_user(&pool, player0_id, player0_balance).await?;
                        db::update_user(&pool, player1_id, player1_balance).await?;
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
                            single_bet_size,
                        };

                        println!("Taking the write lock");

                        // Store game state and channel
                        {
                            let mut games_write = self.games_shared.games.write().await;
                            games_write.insert(game_id.clone(), game_state.clone());
                        }
                        let mut game_channels_write = self.games_shared.game_channels.write().await;
                        game_channels_write.insert(game_id.clone(), tx.clone());

                        let mut player_streams_write =
                            self.games_shared.player_streams.write().await;
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
                GameMessage::Stop { game_id, abort } => {
                    let mut games_write = self.games_shared.games.write().await;
                    if !abort {
                        // Meaning the other person has won
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
                                    winner_idx: (*turn_idx + 1) % 2,
                                    board: board.clone(),
                                    players: players.clone(),
                                    single_bet_size: single_bet_size.clone(),
                                };
                                // Get the channel to broadcast game state
                                let game_channels_read =
                                    self.games_shared.game_channels.read().await;
                                if let Some(channel) = game_channels_read.get(&game_id) {
                                    let response = GameMessage::GameUpdate(game_state.clone());
                                    channel.send(response).await?;
                                }
                            }
                        }
                    } else {
                        if let Some(game_state) = games_write.get_mut(&game_id) {
                            if let GameState::RUNNING {
                                players,
                                single_bet_size,
                                ..
                            } = game_state
                            {
                                let pool = establish_connection().await;
                                let player0_id: i32 = players[0].id.parse()?;
                                let player0 = db::get_user(&pool, player0_id).await?;

                                let player1_id: i32 = players[1].id.parse()?;
                                let player1 = db::get_user(&pool, player1_id).await?;

                                let player0_balance =
                                    player0.wallet_amount + *single_bet_size as i32;
                                let player1_balance =
                                    player1.wallet_amount + *single_bet_size as i32;
                                db::update_user(&pool, player0_id, player0_balance).await?;
                                db::update_user(&pool, player1_id, player1_balance).await?;
                            }

                            *game_state = GameState::ABORTED {
                                game_id: game_id.clone(),
                            };

                            let player_streams = self
                                .games_shared
                                .player_streams
                                .read()
                                .await
                                .get(&game_id)
                                .cloned();

                            if let Some(player_streams) = player_streams {
                                let response = GameMessage::GameUpdate(game_state.clone());
                                println!("Response: {:?}", response);

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
                    }
                }
                GameMessage::MakeMove { game_id, x, y } => {
                    let mut games_write = self.games_shared.games.write().await;

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
                                    let winner = (*turn_idx + 1) % 2;
                                    *game_state = GameState::FINISHED {
                                        game_id: game_id.clone(),
                                        winner_idx: winner,
                                        board: board.clone(),
                                        players: players.clone(),
                                        single_bet_size: single_bet_size.clone(),
                                    };
                                } else {
                                    // Switch turns
                                    *turn_idx = (*turn_idx + 1) % 2;
                                }

                                // Get the channel to broadcast game state
                                let game_channels_read =
                                    self.games_shared.game_channels.read().await;
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
                        single_bet_size,
                    } => {
                        println!("In running");
                        println!("{single_bet_size}. {game_id}");
                        let player_streams = self
                            .games_shared
                            .player_streams
                            .read()
                            .await
                            .get(&game_id)
                            .cloned();

                        if let Some(player_streams) = player_streams {
                            let response = GameMessage::GameUpdate(GameState::RUNNING {
                                game_id,
                                players,
                                board,
                                turn_idx,
                                single_bet_size,
                            });
                            println!("Response: {:?}", response);

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
                        println!("Running msg sent");
                    }
                    GameState::FINISHED {
                        game_id,
                        winner_idx,
                        board,
                        players,
                        single_bet_size,
                    } => {
                        println!("Game finished");
                        let player_streams = self
                            .games_shared
                            .player_streams
                            .read()
                            .await
                            .get(&game_id)
                            .cloned();

                        if let Some(player_streams) = player_streams {
                            let response = GameMessage::GameUpdate(GameState::FINISHED {
                                game_id,
                                winner_idx,
                                board,
                                players: players.clone(),
                                single_bet_size,
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

                        // TODO can try to make just two db calls instead of first deducting single bet size from both users and then addding 2* betsize in one
                        let pool = establish_connection().await;
                        let winner_user_id: i32 = players[winner_idx].id.parse()?;
                        let winner = db::get_user(&pool, winner_user_id).await?;

                        let winner_balance = winner.wallet_amount + 2 * (single_bet_size as i32);
                        db::update_user(&pool, winner_user_id, winner_balance).await?;
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

// // Client-side example (pseudo-code)
// pub struct GameClient {
//     ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,

//     pub player_id: String,
// }

// impl GameClient {
//     pub async fn new(url: &str) -> anyhow::Result<GameClient> {
//         let uri = Uri::try_from(url)?;

//         let (client, _) = ClientBuilder::from_uri(uri).connect().await?;
//         Ok(GameClient {
//             ws_stream: client,
//             player_id: Uuid::new_v4().to_string(),
//         })
//     }

//     pub async fn run_client(self) -> anyhow::Result<()> {
//         // Split the WebSocket stream first
//         let (mut ws_write, mut ws_read) = self.ws_stream.split();
//         let (tx, mut rx) = mpsc::channel(100);

//         // Spawn message handler task first to ensure we don't miss any messages
//         let msg_handler = tokio::spawn({
//             let tx_clone = tx.clone();
//             async move {
//                 while let Some(msg) = ws_read.next().await {
//                     match msg {
//                         Ok(message) => {
//                             println!("Received message from server");
//                             match serde_json::from_slice(message.as_payload()) {
//                                 Ok(response) => {
//                                     if let Err(e) = tx_clone.send(response).await {
//                                         eprintln!("Error sending to channel: {}", e);
//                                         break;
//                                     }
//                                 }
//                                 Err(e) => {
//                                     eprintln!("Error deserializing message: {}", e);
//                                     continue;
//                                 }
//                             }
//                         }
//                         Err(e) => {
//                             eprintln!("WebSocket error: {}", e);
//                             break;
//                         }
//                     }
//                 }
//                 println!("Message handler loop ended");
//                 anyhow::Ok(())
//             }
//         });

//         // Now that the message handler is set up, send the initial game message

//         let play_msg = GameMessage::Play {
//             player_id: self.player_id.clone(),

//         };
//         ws_write
//             .send(Message::binary(serde_json::to_vec(&play_msg)?))
//             .await?;
//         println!("GameMessage::Play message sent");

//         // Main game loop
//         tokio::select! {
//             result = msg_handler => {
//                 if let Err(e) = result {
//                     eprintln!("Message handler error: {}", e);
//                 }
//                 println!("WebSocket connection closed");
//             }
//             result = async {
//                 while let Some(message) = rx.recv().await {
//                     println!("Processing message: {:?}", message);
//                     match &message {
//                         GameMessage::GameUpdate(game_state) => {
//                             match game_state {
//                                 GameState::RUNNING { players, board, turn_idx, game_id } => {
//                                     println!("\nCurrent game state:");
//                                     board.display();
//                                     println!("Game ID: {}", game_id);
//                                     if players[*turn_idx].id == self.player_id {
//                                         println!("\nYour turn! Enter coordinates (x y):");
//                                         let mut input = String::new();
//                                         io::stdin().read_line(&mut input)?;
//                                         let coords: Vec<&str> = input.trim().split_whitespace().collect();

//                                         if coords.len() == 2 {
//                                             if let (Ok(x), Ok(y)) = (coords[0].parse(), coords[1].parse()) {
//                                                 let move_msg = GameMessage::MakeMove {
//                                                     game_id: game_id.clone(),
//                                                     x,
//                                                     y,
//                                                 };
//                                                 println!("Sending move: {:?}", move_msg);
//                                                 ws_write
//                                                     .send(Message::binary(serde_json::to_vec(&move_msg)?))
//                                                     .await?;
//                                             } else {
//                                                 println!("Invalid coordinates! Please enter numbers.");
//                                             }
//                                         } else {
//                                             println!("Invalid input! Please enter two numbers separated by space.");
//                                         }
//                                     } else {
//                                         println!("\nWaiting for other player's move...");
//                                     }
//                                 }
//                                 GameState::WAITING { game_id, .. } => {
//                                     println!("Waiting for other player to join...");
//                                     println!("Game ID: {}", game_id);
//                                 }
//                                 GameState::FINISHED { winner_idx, board, players, .. } => {
//                                     println!("\nGame Over!");
//                                     board.display();
//                                     println!("Winner: Player {} ({})", winner_idx + 1, players[*winner_idx].id);
//                                     return Ok(());
//                                 }
//                             }
//                         }
//                         GameMessage::Error(err) => {
//                             eprintln!("Game error: {}", err);
//                         }
//                         _ => {}
//                     }
//                 }
//                 Ok::<(), anyhow::Error>(())
//             } => {}
//         }

//         Ok(())
//     }
// }
