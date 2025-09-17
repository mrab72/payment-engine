pub mod account;
pub mod transaction;
pub mod payment_engine;
use rust_decimal::Decimal;
use transaction::{TxId};
use thiserror::Error;
pub type Amount = Decimal;


/// Custom error type for payment processing errors.
/// Includes errors for account issues, transaction problems, and invalid operations.
/// Each variant provides a descriptive message for easier debugging and user feedback.
#[derive(Error, Debug)]
pub enum PaymentsError {
    #[error("Failed to parse CSV: {0}")]
    CsvError(#[from] csv::Error),
    #[error("Decimal conversion error: {0}")]
    DecimalError(#[from] rust_decimal::Error),
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Account is frozen due to chargeback")]
    AccountFrozen,
    #[error("Insufficient funds for withdrawal")]
    InsufficientFunds,
    #[error("Transaction not found")]
    TransactionNotFound,
    #[error("Transaction already disputed: {0}")]
    TransactionAlreadyDisputed(TxId),
    #[error("Transaction is not under dispute")]
    TransactionNotDisputed,
    #[error("Client ID mismatch")]
    ClientIdMismatch,
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),
}
