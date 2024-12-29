use colored::Colorize;
use serde::{Deserialize, Serialize};

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
    bomb_coordinates: Vec<u64>,
}

impl Board {
    pub fn new(n: usize) -> Board {
        let bomb_coords = get_bomb_coords(rand::random::<u64>() % 25, 5);

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
        println!("╔{}╗", "═".repeat(self.n * 5));
        for (row_idx, row) in self.grid.iter().enumerate() {
            // Start of row
            print!("║ ");

            for (_, cell) in row.iter().enumerate() {
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
                println!("║ {}", row_idx)
            } else {
                println!("║ {}\n\n", row_idx);
            }
        }

        // Bottom border with column indices
        print!("╚{}╝\n  ", "═".repeat(self.n * 5));

        // Column indices
        for col in 0..self.n {
            print!("{:<3} ", col);
        }
        println!();
    }
}
