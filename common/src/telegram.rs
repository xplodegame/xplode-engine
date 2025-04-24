use anyhow::Result;
use serde::Serialize;
use std::env;
use tracing::{error, info};

const TELEGRAM_API_URL: &str = "https://api.telegram.org/bot";

#[derive(Serialize)]
struct SendMessageRequest {
    chat_id: String,
    text: String,
}

pub async fn send_telegram_message(message: &str) -> Result<()> {
    let bot_token = "7480417645:AAFEizy5dQuCWGDez843s2kLUQeiiLIf2WE";
    let chat_id = "-1002545187878"; // Your private chat ID

    let client = reqwest::Client::new();
    let url = format!("{}{}/sendMessage", TELEGRAM_API_URL, bot_token);

    let request = SendMessageRequest {
        chat_id: chat_id.to_string(),
        text: message.to_string(),
    };

    info!("Sending telegram message: {}", message);
    info!("Using bot token: {}", bot_token);
    info!("Using chat ID: {}", chat_id);

    let response = client.post(&url).json(&request).send().await?;
    info!("Telegram API response status: {}", response.status());

    if !response.status().is_success() {
        let error_text = response.text().await?;
        error!("Telegram API error: {}", error_text);
    }

    Ok(())
}
