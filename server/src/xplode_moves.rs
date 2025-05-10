use anyhow::Result;
use reqwest::Client as HttpClient;
use serde_json::json;

#[derive(Clone)]
pub struct XplodeMovesClient {
    api_base: String,
    client: HttpClient,
}

impl XplodeMovesClient {
    pub fn new(api_base: String) -> Self {
        Self {
            api_base,
            client: HttpClient::new(),
        }
    }

    pub async fn initialize_game(
        &self,
        game_id: &str,
        grid_size: u32,
        bomb_positions: Vec<(usize, usize)>,
    ) -> Result<String> {
        let bomb_positions: Vec<_> = bomb_positions
            .into_iter()
            .map(|(x, y)| json!({ "x": x, "y": y }))
            .collect();

        let response = self
            .client
            .post(&format!("{}/initialize", self.api_base))
            .json(&json!({
                "gameId": game_id,
                "gridSize": grid_size,
                "bombPositions": bomb_positions
            }))
            .send()
            .await?;

        let result = response.json::<serde_json::Value>().await?;
        Ok(result["transaction"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    pub async fn record_move(
        &self,
        game_id: &str,
        player_name: &str,
        x: usize,
        y: usize,
    ) -> Result<String> {
        let response = self
            .client
            .post(&format!("{}/move", self.api_base))
            .json(&json!({
                "gameId": game_id,
                "playerName": player_name,
                "cell": { "x": x, "y": y }
            }))
            .send()
            .await?;

        let result = response.json::<serde_json::Value>().await?;
        Ok(result["transaction"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    pub async fn commit_game(&self, game_id: &str) -> Result<String> {
        println!("Committing game on blockchain");
        let response = self
            .client
            .post(&format!("{}/commit", self.api_base))
            .json(&json!({
                "gameId": game_id
            }))
            .send()
            .await?;

        let result = response.json::<serde_json::Value>().await?;
        Ok(result["transaction"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }
}
