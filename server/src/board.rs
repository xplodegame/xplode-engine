use colored::Colorize;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::seed_gen::get_bomb_coords;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CellState {
    Mined,
    Hidden,
    Bomb,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    n: usize, // it would be nXn
    grid: Vec<Vec<CellState>>,
    //TODO: It should be either continuous or scattered
    bomb_coordinates: Vec<u64>,
}

impl Board {
    pub fn new(n: usize, bombs: usize) -> Board {
        let bomb_coords = get_bomb_coords(bombs, n as u64);

        Board {
            n,
            grid: vec![vec![CellState::Hidden; n]; n],
            bomb_coordinates: bomb_coords,
        }
    }

    pub fn mine(&mut self, x: usize, y: usize) -> bool {
        let position = x * self.n + y;
        if self.bomb_coordinates.contains(&(position as u64)) {
            self.grid[x][y] = CellState::Bomb;
            true // true means bomb
        } else {
            self.grid[x][y] = CellState::Mined;
            false
        }
    }

    pub fn display(&self) {
        info!("╔{}╗", "═".repeat(self.n * 5));
        for (row_idx, row) in self.grid.iter().enumerate() {
            // Start of row
            print!("║ ");

            for cell in row.iter() {
                match cell {
                    CellState::Mined => {
                        // Diamond with optional value
                        print!("{:<3} ", "💎".green());
                    }
                    CellState::Hidden => {
                        // Hidden cell with optional hint
                        print!("{:<3} ", "😑".blue());
                    }
                    CellState::Bomb => {
                        // Special cell with distinct marking

                        print!("{:<3} ", "💣".yellow());
                    }
                }
            }

            // Row number on the right side
            if row_idx == self.n - 1 {
                info!("║ {}", row_idx)
            } else {
                info!("║ {}\n\n", row_idx);
            }
        }

        // Bottom border with column indices
        print!("╚{}╝\n  ", "═".repeat(self.n * 5));

        // Column indices
        for col in 0..self.n {
            print!("{:<3} ", col);
        }
    }
}
