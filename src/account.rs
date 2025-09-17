use derive_more::Display;
use serde::{Deserialize, Serialize};

use crate::errors::PaymentsError;
use crate::transaction::Amount;

/// Unique identifier for a client.
pub type ClientId = u16;

/// Represents a client's account with available, held, and total funds, as well as a locked status.
///
#[derive(Debug, Clone, Display, Deserialize, Serialize)]
#[display(
    "Client {}: available={}, held={}, total={}, locked={}",
    client,
    available,
    held,
    total,
    locked
)]
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
    pub fn withdraw(&mut self, amount: Amount) -> Result<(), PaymentsError> {
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
        if self.locked {
            return Err(PaymentsError::AccountFrozen);
        }
        if self.available < amount {
            return Err(PaymentsError::InsufficientFunds);
        }
        self.available -= amount;
        self.held += amount;
        Ok(())
    }

    /// Releases a hold on the specified amount, moving it from held to available funds.
    pub fn release(&mut self, amount: Amount) -> Result<(), PaymentsError> {
        if self.held < amount {
            return Err(PaymentsError::InsufficientFunds);
        }
        self.held -= amount;
        self.available += amount;
        Ok(())
    }

    pub fn chargeback(&mut self, amount: Amount) -> Result<(), PaymentsError> {
        if self.held < amount {
            return Err(PaymentsError::InsufficientFunds);
        }
        self.held -= amount;
        self.total -= amount;
        self.locked = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = Account::new(1);
        assert_eq!(account.client, 1);
        assert_eq!(account.available, Amount::new(0, 0));
        assert_eq!(account.held, Amount::new(0, 0));
        assert_eq!(account.total, Amount::new(0, 0));
        assert!(!account.locked);
    }

    #[test]
    fn test_deposit() {
        let mut account = Account::new(1);
        account.deposit(Amount::new(100, 0)).unwrap();
        assert_eq!(account.available, Amount::new(100, 0));
        assert_eq!(account.total, Amount::new(100, 0));
    }

    #[test]
    fn test_withdraw() {
        let mut account = Account::new(1);
        account.deposit(Amount::new(100, 0)).unwrap();
        account.withdraw(Amount::new(50, 0)).unwrap();
        assert_eq!(account.available, Amount::new(50, 0));
        assert_eq!(account.total, Amount::new(50, 0));
    }

    #[test]
    fn test_withdraw_insufficient_funds() {
        let mut account = Account::new(1);
        let result = account.withdraw(Amount::new(50, 0));
        assert!(matches!(result, Err(PaymentsError::InsufficientFunds)));
    }

    #[test]
    fn test_hold() {
        let mut account = Account::new(1);
        account.deposit(Amount::new(100, 0)).unwrap();
        account.hold(Amount::new(30, 0)).unwrap();
        assert_eq!(account.available, Amount::new(70, 0));
        assert_eq!(account.held, Amount::new(30, 0));
        assert_eq!(account.total, Amount::new(100, 0));
    }

    #[test]
    fn test_release() {
        let mut account = Account::new(1);
        account.deposit(Amount::new(100, 0)).unwrap();
        account.hold(Amount::new(30, 0)).unwrap();
        account.release(Amount::new(20, 0)).unwrap();
        assert_eq!(account.available, Amount::new(90, 0));
        assert_eq!(account.held, Amount::new(10, 0));
        assert_eq!(account.total, Amount::new(100, 0));
    }

    #[test]
    fn test_chargeback() {
        let mut account = Account::new(1);
        account.deposit(Amount::new(100, 0)).unwrap();
        account.hold(Amount::new(50, 0)).unwrap();
        account.chargeback(Amount::new(50, 0)).unwrap();
        assert_eq!(account.available, Amount::new(50, 0));
        assert_eq!(account.held, Amount::new(0, 0));
        assert_eq!(account.total, Amount::new(50, 0));
        assert!(account.locked);
    }

    #[test]
    fn test_account_locked() {
        let mut account = Account::new(1);
        account.locked = true;
        let deposit_result = account.deposit(Amount::new(100, 0));
        assert!(matches!(deposit_result, Err(PaymentsError::AccountFrozen)));
        let withdraw_result = account.withdraw(Amount::new(50, 0));
        assert!(matches!(withdraw_result, Err(PaymentsError::AccountFrozen)));
        let hold_result = account.hold(Amount::new(30, 0));
        assert!(matches!(hold_result, Err(PaymentsError::AccountFrozen)));
    }
}
