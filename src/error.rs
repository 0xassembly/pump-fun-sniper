use thiserror::Error;
use solana_client::client_error::ClientError;
use sqlx::Error as SqlxError;

#[derive(Error, Debug)]
pub enum BotError {
    #[error("Solana client error: {0}")]
    ClientError(#[from] ClientError),

    #[error("Database error: {0}")]
    DatabaseError(#[from] SqlxError),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Account error: {0}")]
    AccountError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Metrics error: {0}")]
    MetricsError(String),
} 