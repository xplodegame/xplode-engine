use dotenv::dotenv;
use serde::Deserialize;
use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    // Server configuration
    pub server_host: String,
    pub server_port: u16,

    // Database configuration
    pub database_url: String,

    // Security
    pub jwt_secret: String,
    pub jwt_expiration: u64, // in seconds
    pub allowed_origins: Vec<String>,
    pub rate_limit: u32,

    // Transfer configuration
    pub withdrawal_min_amount: f64,
    pub withdrawal_max_amount: f64,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
        dotenv().ok();

        let server_host = env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let server_port = env::var("SERVER_PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()
            .expect("SERVER_PORT must be a valid port number");

        let database_url = env::var("DATABASE_URL")?;

        let jwt_secret =
            env::var("JWT_SECRET").expect("JWT_SECRET must be set for secure operation");

        let jwt_expiration = env::var("JWT_EXPIRATION")
            .unwrap_or_else(|_| "86400".to_string()) // 24 hours by default
            .parse::<u64>()
            .expect("JWT_EXPIRATION must be a valid number of seconds");

        let allowed_origins = env::var("ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:3000".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let rate_limit = env::var("RATE_LIMIT")
            .unwrap_or_else(|_| "60".to_string())
            .parse::<u32>()
            .expect("RATE_LIMIT must be a valid number");

        let withdrawal_min_amount = env::var("WITHDRAWAL_MIN_AMOUNT")
            .unwrap_or_else(|_| "0.001".to_string())
            .parse::<f64>()
            .expect("WITHDRAWAL_MIN_AMOUNT must be a valid number");

        let withdrawal_max_amount = env::var("WITHDRAWAL_MAX_AMOUNT")
            .unwrap_or_else(|_| "1000.0".to_string())
            .parse::<f64>()
            .expect("WITHDRAWAL_MAX_AMOUNT must be a valid number");

        Ok(Config {
            server_host,
            server_port,
            database_url,
            jwt_secret,
            jwt_expiration,
            allowed_origins,
            rate_limit,
            withdrawal_min_amount,
            withdrawal_max_amount,
        })
    }

    pub fn server_address(&self) -> String {
        format!("{}:{}", self.server_host, self.server_port)
    }
}
