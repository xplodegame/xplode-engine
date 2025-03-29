use lazy_static::lazy_static;
use prometheus::{
    register_histogram_vec, register_int_counter, register_int_counter_vec, register_int_gauge,
    register_int_gauge_vec, Histogram, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec,
};

// System Metrics
lazy_static! {
    pub static ref CPU_USAGE: IntGauge =
        register_int_gauge!("cpu_usage_percent", "Current CPU usage percentage").unwrap();
    pub static ref MEMORY_USAGE: IntGauge =
        register_int_gauge!("memory_usage_bytes", "Current memory usage in bytes").unwrap();
    pub static ref ACTIVE_CONNECTIONS: IntGauge = register_int_gauge!(
        "active_connections",
        "Number of active WebSocket connections"
    )
    .unwrap();
}

// Game Metrics
lazy_static! {
    pub static ref ACTIVE_GAMES: IntGauge =
        register_int_gauge!("active_games", "Number of currently active games").unwrap();
    pub static ref TOTAL_PLAYERS_ONLINE: IntGauge = register_int_gauge!(
        "total_players_online",
        "Total number of players currently online"
    )
    .unwrap();
    pub static ref GAMES_COMPLETED: IntCounter =
        register_int_counter!("games_completed_total", "Total number of completed games").unwrap();
    pub static ref GAMES_ABANDONED: IntCounter =
        register_int_counter!("games_abandoned_total", "Total number of abandoned games").unwrap();
    pub static ref GAME_DURATION: HistogramVec = register_histogram_vec!(
        "game_duration_seconds",
        "Distribution of game durations",
        &["game_type"],
        vec![30.0, 60.0, 120.0, 300.0, 600.0]
    )
    .unwrap();
}

// Transaction Metrics
lazy_static! {
    pub static ref GAME_CREATION_COUNTER: IntCounter =
        register_int_counter!("game_creations_total", "Total number of games created").unwrap();
    pub static ref REWARD_DISTRIBUTION: IntCounterVec = register_int_counter_vec!(
        "rewards_distributed_total",
        "Total rewards distributed",
        &["game_type"]
    )
    .unwrap();
    pub static ref TRANSACTION_PROCESSING_TIME: HistogramVec = register_histogram_vec!(
        "transaction_processing_seconds",
        "Time taken to process transactions",
        &["transaction_type"],
        vec![0.1, 0.5, 1.0, 2.0, 5.0]
    )
    .unwrap();
}

// API Performance Metrics
lazy_static! {
    pub static ref HTTP_REQUESTS_TOTAL: IntCounterVec = register_int_counter_vec!(
        "http_requests_total",
        "Total number of HTTP requests",
        &["endpoint", "method", "status"]
    )
    .unwrap();
    pub static ref HTTP_REQUEST_DURATION: HistogramVec = register_histogram_vec!(
        "http_request_duration_seconds",
        "HTTP request duration in seconds",
        &["endpoint", "method"],
        vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
    )
    .unwrap();
    pub static ref WEBSOCKET_MESSAGES: IntCounterVec = register_int_counter_vec!(
        "websocket_messages_total",
        "Total WebSocket messages",
        &["message_type"]
    )
    .unwrap();
}

// Helper functions to update metrics
pub fn record_game_start() {
    ACTIVE_GAMES.inc();
    GAME_CREATION_COUNTER.inc();
}

pub fn record_game_end(duration_secs: f64, game_type: &str) {
    ACTIVE_GAMES.dec();
    GAMES_COMPLETED.inc();
    GAME_DURATION
        .with_label_values(&[game_type])
        .observe(duration_secs);
}

pub fn record_game_abandon() {
    ACTIVE_GAMES.dec();
    GAMES_ABANDONED.inc();
}

pub fn record_player_connection() {
    ACTIVE_CONNECTIONS.inc();
    TOTAL_PLAYERS_ONLINE.inc();
}

pub fn record_player_disconnection() {
    ACTIVE_CONNECTIONS.dec();
    TOTAL_PLAYERS_ONLINE.dec();
}

pub fn record_http_request(endpoint: &str, method: &str, status: &str, duration: f64) {
    HTTP_REQUESTS_TOTAL
        .with_label_values(&[endpoint, method, status])
        .inc();
    HTTP_REQUEST_DURATION
        .with_label_values(&[endpoint, method])
        .observe(duration);
}

pub fn record_websocket_message(message_type: &str) {
    WEBSOCKET_MESSAGES.with_label_values(&[message_type]).inc();
}

pub fn record_reward_distribution(game_type: &str) {
    REWARD_DISTRIBUTION.with_label_values(&[game_type]).inc();
}

pub fn record_transaction_processing_time(transaction_type: &str, duration: f64) {
    TRANSACTION_PROCESSING_TIME
        .with_label_values(&[transaction_type])
        .observe(duration);
}

// System metrics update functions
pub fn update_cpu_usage(usage: i64) {
    CPU_USAGE.set(usage);
}

pub fn update_memory_usage(bytes: i64) {
    MEMORY_USAGE.set(bytes);
}
