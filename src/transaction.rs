use crate::account::ClientId;
use rust_decimal::Decimal;
use serde::Deserialize;
use derive_more::Display;

pub type Amount = Decimal;

pub type TxId = u32;
/// Transaction types supported by the payment engine.
/// The `serde` attribute ensures that the enum variants are deserialized
/// from lowercase strings in the input data.
#[derive(Debug, Clone, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    /// A deposit transaction.
    Deposit,

    /// A withdrawal transaction.
    Withdrawal,

    /// A dispute transaction.
    Dispute,

    /// A resolve transaction.
    Resolve,

    /// A chargeback transaction.
    Chargeback,
}

#[derive(Debug, Clone, Deserialize, Display)]
#[display("Transaction {{ type: {}, client: {}, tx: {}, amount: {:?} }}", tx_type, client, tx, amount)]
pub struct Transaction {
    /// The type of transaction.
    #[serde(rename = "type")]
    pub tx_type: TransactionType,

    /// The client associated with the transaction.
    pub client: u16,

    /// The unique identifier for the transaction.
    pub tx: TxId,

    /// The amount involved in the transaction (if applicable).
    pub amount: Option<Amount>,
}

/// Represents a stored transaction with its details.
#[derive(Debug, Clone, Deserialize)]
pub struct StoredTransaction {
    /// Unique identifier for the client.
    pub client: ClientId,

    /// The amount involved in the transaction.
    pub amount: Amount,

    /// Indicates if the transaction is currently disputed.
    pub disputed: bool,
}
