
use derive_more::Display;
use serde::{Deserialize, Serialize};

use super::{Amount, PaymentsError};

/// Unique identifier for a client.
pub type ClientId = u16;

/// Represents a client's account with available, held, and total funds, as well as a locked status.
/// 
#[derive(Debug, Clone, Display, Deserialize, Serialize)] 
#[display("Client {}: available={}, held={}, total={}, locked={}", client, available, held, total, locked)]
pub struct Account {
    /// Unique identifier for the client.
    
    pub client: ClientId,

    /// Funds available for transactions.
    #[serde(with = "rust_decimal::serde::str")]
    pub available: Amount,

    /// Funds held due to disputes.
    #[serde(with = "rust_decimal::serde::str")]
    pub held: Amount,

    /// Total funds (available + held).
    #[serde(with = "rust_decimal::serde::str")]
    pub total: Amount,

    /// Indicates if the account is locked (e.g., after a chargeback).
    pub locked: bool,
}

impl Account {
    /// Creates a new account for the given client ID with zero balances and unlocked status.
    pub fn new(client: ClientId) -> Self {
        Self {
            client,
            available: Amount::new(0, 0),
            held: Amount::new(0, 0),
            total: Amount::new(0, 0),
            locked: false,
        }
    }

    /// Deposits the specified amount into the account, updating available and total balances.
    /// Returns an error if the account is locked.
    pub fn deposit(&mut self, amount: Amount) -> Result<(), PaymentsError> {
        if self.locked {
            return Err(PaymentsError::AccountFrozen);
        }
        
        self.available += amount;
        self.total += amount;
        Ok(())
    }

    /// Withdraws the specified amount from the account, updating available and total balances.
    /// Returns an error if the account is locked or if there are insufficient funds.
    pub fn withdraw(&mut self, amount: Amount) -> Result<(), PaymentsError>{
        if self.locked {
            return Err(PaymentsError::AccountFrozen);
        }
        
        if self.available < amount {
            return Err(PaymentsError::InsufficientFunds);
        }
        
        self.available -= amount;
        self.total -= amount;
        Ok(())
    }

    /// Places a hold on the specified amount, moving it from available to held funds.
    /// Returns an error if the account is locked or if there are insufficient available funds.
    pub fn hold(&mut self, amount: Amount) -> Result<(), PaymentsError> {
        if self.available < amount {
            return Err(PaymentsError::InsufficientFunds);
        }
        self.available -= amount;
        self.held += amount;
        Ok(())
    }

    /// Releases a hold on the specified amount, moving it from held to available funds.
    pub fn release(&mut self, amount: Amount) -> Result<(), PaymentsError> {
        self.held -= amount;
        self.available += amount;
        Ok(())
    }

    pub fn chargeback(&mut self, amount: Amount) -> Result<(), PaymentsError> {
        self.held -= amount;
        self.total -= amount;
        self.locked = true;
        Ok(())
    }
}