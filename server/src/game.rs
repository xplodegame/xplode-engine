// use std::{
//     collections::HashMap,
//     io::{self, stdin},
//     process,
//     sync::{Mutex, RwLock},
//     time::Duration,
// };

// use redis::{AsyncCommands, Client};
// use serde::{Deserialize, Serialize};
// use uuid::Uuid;

// use crate::{board::Board, player::Player};

// #[derive(Debug, Clone, Serialize, Deserialize)]
// enum GameState {
//     WAITING {
//         creator: Player,
//         board: Board,
//     },
//     RUNNING {
//         players: Vec<Player>,
//         board: Board,
//         turn_idx: usize,
//     },
//     FINISHED {
//         winner_idx: usize,
//         board: Board,
//         players: Vec<Player>,
//     },
// }

// // lazy_static! {
// //     static ref GAME_STATES: RwLock<HashMap<String, GameState>> = RwLock::new(HashMap::new());
// //     static ref PLAYERS: RwLock<HashMap<u32, Player>> = RwLock::new(HashMap::new()); // mapping process id to player
// // }

// #[derive(Debug)]
// pub struct GameManager {
//     redis: Client,
// }

// impl GameManager {
//     pub fn new() -> Result<Self> {
//         let redis = Client::open("redis://127.0.0.1/")?;
//         Ok(Self { redis })
//     }
//     pub async fn create_game(&self) -> anyhow::Result<String> {
//         println!("Create game");
//         let mut conn = self.redis.get_multiplexed_tokio_connection().await?;
//         let pid = process::id().to_string();
//         let game_id = Uuid::new_v4().to_string();
//         let board = Board::new(5);

//         let mut player = Player::new();
//         let player_str: Option<String> = conn.get(pid.to_string()).await?;
//         if player_str.is_some() {
//             player = serde_json::from_str(&player_str.unwrap())?;
//         } else {
//             conn.set::<String, String, ()>(pid, serde_json::to_string(&player)?)
//                 .await?;
//         }

//         let game_state = GameState::WAITING {
//             creator: player,
//             board,
//         };
//         conn.set::<String, String, ()>(game_id.clone(), serde_json::to_string(&game_state)?)
//             .await?;

//         Ok(game_id)
//     }

//     pub async fn join_game(&self, game_id: String) -> anyhow::Result<()> {
//         let pid = process::id().to_string();

//         let mut conn = self.redis.get_multiplexed_tokio_connection().await?;

//         let mut player = Player::new();
//         let player_str: Option<String> = conn.get(&pid).await?;
//         if player_str.is_some() {
//             player = serde_json::from_str(&player_str.unwrap()).unwrap();
//         } else {
//             conn.set::<String, String, ()>(pid, serde_json::to_string(&player)?)
//                 .await?;
//         }

//         let game_state_str: String = conn.get(&game_id).await?;
//         let game_state: GameState = serde_json::from_str(&game_state_str)?;

//         if let GameState::WAITING { creator, board } = game_state {
//             let players = vec![creator, player];
//             let new_game_state = GameState::RUNNING {
//                 players,
//                 board,
//                 turn_idx: 0,
//             };

//             conn.set::<String, String, ()>(
//                 game_id.clone(),
//                 serde_json::to_string(&new_game_state)?,
//             )
//             .await?;
//         }

//         Ok(())
//     }

//     pub async fn start_game(&self, game_id: String) -> anyhow::Result<()> {
//         loop {
//             let mut conn = self.redis.get_multiplexed_tokio_connection().await?;
//             let game_state_str: String = conn.get(&game_id).await?;
//             let game_state = serde_json::from_str(&game_state_str)?;

//             match game_state {
//                 GameState::WAITING { .. } => {
//                     tokio::time::sleep(Duration::from_millis(100)).await;
//                     continue;
//                 }
//                 GameState::RUNNING {
//                     players,
//                     board,
//                     turn_idx,
//                 } => {
//                     if players[turn_idx].id == std::process::id().to_string() {
//                         board.display();
//                         println!("X and Y seperated by a space\n");
//                         let mut input = String::new();
//                         io::stdin()
//                             .read_line(&mut input) // Read the line and store it in 'input'
//                             .expect("Failed to read line"); // Handle any potential errors

//                         // Trim the input to remove any trailing newline characters
//                         let trimmed_input = input.trim();
//                         let coords: Vec<&str> = trimmed_input.split_whitespace().collect();
//                         let x = coords[0].parse()?; // First coordinate
//                         let y = coords[1].parse()?;
//                         let mut new_board = board.clone();
//                         if new_board.mine(x, y) {
//                             let winner = (turn_idx + 1) % 2;
//                             let new_game_state = GameState::FINISHED {
//                                 winner_idx: winner,
//                                 board: new_board,
//                                 players,
//                             };
//                             conn.set::<String, String, ()>(
//                                 game_id.clone(),
//                                 serde_json::to_string(&new_game_state)?,
//                             )
//                             .await?;
//                             continue;
//                         }
//                         let next_turn = (turn_idx + 1) % 2;
//                         let new_game_state = GameState::RUNNING {
//                             players,
//                             board: new_board.clone(),
//                             turn_idx: next_turn,
//                         };

//                         conn.set::<String, String, ()>(
//                             game_id.clone(),
//                             serde_json::to_string(&new_game_state)?,
//                         )
//                         .await?;
//                         new_board.display();
//                     } else {
//                         tokio::time::sleep(Duration::from_millis(100)).await;
//                     }
//                     continue;
//                 }
//                 GameState::FINISHED {
//                     winner_idx,
//                     board,
//                     players,
//                 } => {
//                     board.display();
//                     if players[winner_idx].id == process::id().to_string() {
//                         println!("You won");
//                     } else {
//                         println!("You lost");
//                     }
//                     break;
//                 }
//             }
//         }

//         Ok(())
//     }
// }

// // player take turns until one of them wins
