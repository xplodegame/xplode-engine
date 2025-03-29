use prometheus_client::{
    metrics::{counter::Counter, gauge::Gauge, histogram::Histogram},
    registry::Registry,
};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Metrics {
    // Counters
    pub total_deposits: Counter,
    pub total_withdrawals: Counter,
    pub failed_transactions: Counter,

    // Gauges
    pub active_users: Gauge,
    pub total_wallet_balance: Gauge,

    // Histograms
    pub transaction_amount: Histogram,
    pub api_latency: Histogram,
}

impl Metrics {
    pub fn new(registry: &mut Registry) -> Self {
        // Initialize counters
        let total_deposits = Counter::default();
        let total_withdrawals = Counter::default();
        let failed_transactions = Counter::default();

        // Initialize gauges
        let active_users = Gauge::default();
        let total_wallet_balance = Gauge::default();

        // Initialize histograms
        let transaction_amount = Histogram::new(
            vec![0.0, 10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0].into_iter(),
        );
        let api_latency =
            Histogram::new(vec![0.0, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0].into_iter());

        // Register metrics
        registry.register(
            "total_deposits",
            "Total number of deposits",
            total_deposits.clone(),
        );
        registry.register(
            "total_withdrawals",
            "Total number of withdrawals",
            total_withdrawals.clone(),
        );
        registry.register(
            "failed_transactions",
            "Total number of failed transactions",
            failed_transactions.clone(),
        );
        registry.register(
            "active_users",
            "Number of active users",
            active_users.clone(),
        );
        registry.register(
            "total_wallet_balance",
            "Total wallet balance across all users",
            total_wallet_balance.clone(),
        );
        registry.register(
            "transaction_amount",
            "Distribution of transaction amounts",
            transaction_amount.clone(),
        );
        registry.register(
            "api_latency",
            "API endpoint latency in seconds",
            api_latency.clone(),
        );

        Self {
            total_deposits,
            total_withdrawals,
            failed_transactions,
            active_users,
            total_wallet_balance,
            transaction_amount,
            api_latency,
        }
    }
}

pub type SharedMetrics = Arc<RwLock<Metrics>>;
