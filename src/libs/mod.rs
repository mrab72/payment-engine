pub mod account;
pub mod transaction;
pub mod payment_engine;
use rust_decimal::Decimal;
use thiserror::Error;
pub type Amount = Decimal;


#[derive(Error, Debug)]
pub enum PaymentsError {
    #[error("Account is frozen due to chargeback")]
    AccountFrozen,
    #[error("Insufficient funds for withdrawal")]
    InsufficientFunds,
    #[error("Transaction not found")]
    TransactionNotFound,
    #[error("Transaction already disputed")]
    TransactionAlreadyDisputed,
    #[error("Transaction is not under dispute")]
    TransactionNotDisputed,
    #[error("Client ID mismatch")]
    ClientIdMismatch,
}
