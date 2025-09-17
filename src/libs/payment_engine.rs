use std::collections::HashMap;
use rust_decimal::Decimal;
use serde::Deserialize;

use super::account::{Account, ClientId};
use super::transaction::{TxId, StoredTransaction};

/// Core payment engine struct that manages client accounts and transactions.
/// It supports processing various transaction types including deposits, withdrawals, disputes, resolutions, and chargebacks.
/// It maintains a mapping of client IDs to their respective accounts and a record of all transactions processed.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PaymentsEngine {
    /// Mapping of client IDs to their accounts.
    accounts: HashMap<ClientId, Account>,

    /// Record of all transactions processed, keyed by transaction ID.
    transactions: HashMap<TxId, StoredTransaction>,
}



impl PaymentsEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retrieves an existing account or creates a new one if it doesn't exist.
    pub fn get_or_create_account(&mut self, client_id: ClientId) -> &mut Account {
        self.accounts.entry(client_id).or_insert_with(|| Account::new(client_id))
    }

    pub fn process_transaction(&mut self, transaction: &super::transaction::Transaction) -> Result<(), super::PaymentsError> {
        match transaction.tx_type {
            super::transaction::TransactionType::Deposit => self.process_deposit(transaction),
            super::transaction::TransactionType::Withdrawal => self.process_withdrawal(transaction),
            super::transaction::TransactionType::Dispute => self.process_dispute(transaction),
            super::transaction::TransactionType::Resolve => self.process_resolve(transaction),
            super::transaction::TransactionType::Chargeback => self.process_chargeback(transaction),
        }
    }

    fn process_deposit(&mut self, transaction: &super::transaction::Transaction) -> Result<(), super::PaymentsError> {
        let amount = transaction.amount.ok_or(super::PaymentsError::InvalidTransaction("Deposit transaction must have an amount".to_string()))?;
        if amount <= Decimal::ZERO {
            return Err(super::PaymentsError::InvalidTransaction("Deposit amount must be positive".to_string()));
        }
        if self.transactions.contains_key(&transaction.tx) {
            return Err(super::PaymentsError::InvalidTransaction(format!("Transaction ID {} already exists", transaction.tx)));
        }
        let client_id = transaction.client;
        let account = self.get_or_create_account(client_id);
        account.deposit(amount)?;

        self.transactions.insert(transaction.tx, StoredTransaction {
            client: client_id,
            amount,
            disputed: false,
        });

        Ok(())
    }

    fn process_withdrawal(&mut self, transaction: &super::transaction::Transaction) -> Result<(), super::PaymentsError> {
        let amount = transaction.amount.ok_or(super::PaymentsError::InvalidTransaction("Withdrawal transaction must have an amount".to_string()))?;
        if amount <= Decimal::ZERO {
            return Err(super::PaymentsError::InvalidTransaction("Withdrawal amount must be positive".to_string()));
        }
        if self.transactions.contains_key(&transaction.tx) {
            return Err(super::PaymentsError::InvalidTransaction(format!("Transaction ID {} already exists", transaction.tx)));
        }
        let client_id = transaction.client;
        let account = self.get_or_create_account(client_id);
        account.withdraw(amount)?;
        self.transactions.insert(transaction.tx, StoredTransaction {
            client: client_id,
            amount,
            disputed: false,
        });
        Ok(())
    }

    fn process_dispute(&mut self, transaction: &super::transaction::Transaction) -> Result<(), super::PaymentsError> {
        if transaction.amount.is_some() {
            return Err(super::PaymentsError::InvalidTransaction("Dispute transaction should not have an amount".to_string()));
        }

        let (client_id, amount)= {
            let stored_tx = self
                .transactions
                .get_mut(&transaction.tx)
                .ok_or(super::PaymentsError::TransactionNotFound)?;

            if stored_tx.client != transaction.client {
                return Err(super::PaymentsError::ClientIdMismatch);
            }

            if stored_tx.disputed {
                return Err(super::PaymentsError::TransactionAlreadyDisputed(transaction.tx));
            }

            stored_tx.disputed = true;

            (stored_tx.client, stored_tx.amount)
        };

        let account = self.get_or_create_account(client_id);
        account.hold(amount)?;
        Ok(())
    }

    fn process_resolve(&mut self, transaction: &super::transaction::Transaction) -> Result<(), super::PaymentsError> {
        if transaction.amount.is_some() {
            return Err(super::PaymentsError::InvalidTransaction("Resolve transaction should not have an amount".to_string()));
        }
        let (client_id, amount) = {
            let stored_tx = self.transactions.get_mut(&transaction.tx).ok_or(super::PaymentsError::TransactionNotFound)?;
            if stored_tx.client != transaction.client {
                return Err(super::PaymentsError::ClientIdMismatch);
            }

            if !stored_tx.disputed {
                return Err(super::PaymentsError::TransactionNotDisputed);
            }
            stored_tx.disputed = false;
            (stored_tx.client, stored_tx.amount)
        };

        let account = self.get_or_create_account(client_id);
        account.release(amount)?;
        
        Ok(())
    }

    fn process_chargeback(&mut self, transaction: &super::transaction::Transaction) -> Result<(), super::PaymentsError> {
        if transaction.amount.is_some() {
            return Err(super::PaymentsError::InvalidTransaction("Chargeback transaction should not have an amount".to_string()));
        }
        let (client_id, amount) = {
            let stored_tx = self.transactions.get_mut(&transaction.tx).ok_or(super::PaymentsError::TransactionNotFound)?;
            if stored_tx.client != transaction.client {
                return Err(super::PaymentsError::ClientIdMismatch);
            }

            if !stored_tx.disputed {
                return Err(super::PaymentsError::TransactionNotDisputed);
            }
            stored_tx.disputed = false;
            (stored_tx.client, stored_tx.amount)
        };

        let account = self.get_or_create_account(client_id);
        account.chargeback(amount)?;
        
        Ok(())
    }

    /// Processes transactions from a CSV file.
    /// Each line in the CSV is parsed into a Transaction and processed.
    /// Logs errors for any transactions that fail to process.
    pub fn process_transactions_from_file(&mut self, file_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let file = std::fs::File::open(file_path)?;
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(file);

        log::debug!("Starting to process transactions from file: {:?}", file_path);

        for (idx, line) in rdr.deserialize().enumerate() {
            let transaction: super::transaction::Transaction = match line {
                Ok(tx) => tx,
                Err(e) => {
                    log::error!("Failed to parse line {}: {}", idx + 1, e);
                    continue;
                }
            };

            if let Err(e) = self.process_transaction(&transaction) {
                log::error!("Failed to process transaction {:?}: {}", transaction, e);
            } else {
                log::debug!("Successfully processed transaction: {:?}", transaction);
            }
        }
        Ok(())
    }

    /// Writes the current state of all accounts to a CSV file or writer.
    /// If `writer` is None, writes to stdout.
    /// The CSV contains columns: client, available, held, total, locked.
    pub fn write_accounts_csv<W: std::io::Write>(&self, writer: W) -> Result<(), Box<dyn std::error::Error>> {
        let mut wtr = csv::WriterBuilder::new()
            .has_headers(true)
            .from_writer(writer);

        wtr.write_record(["client", "available", "held", "total", "locked"])?;
        
        for account in self.accounts.values() {
            wtr.serialize(account)?;
        }
 
        wtr.flush()?;
        log::info!("Successfully wrote accounts to CSV");
        Ok(())
    }

    pub fn get_accounts(&self) -> Vec<&Account> {
        self.accounts.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libs::PaymentsError;
    use crate::libs::transaction;

    #[test]
    fn test_deposit() {
        let mut engine = PaymentsEngine::new();
        let tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(1000, 2)), // 10.00
        };
        engine.process_transaction(&tx).unwrap();
        let account = engine.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(1000, 2));
        assert_eq!(account.total, Decimal::new(1000, 2));
        assert!(!account.locked);
    }

    #[test]
    fn test_withdrawal() {
        let mut engine = PaymentsEngine::new();
        let deposit_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(1000, 2)),
        };
        engine.process_transaction(&deposit_tx).unwrap();
        let withdrawal_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(500, 2)),
        };
        engine.process_transaction(&withdrawal_tx).unwrap();
        let account = engine.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(500, 2));
        assert_eq!(account.total, Decimal::new(500, 2));
        assert!(!account.locked);
    }

    #[test]
    fn test_dispute_resolve() {
        let mut engine = PaymentsEngine::new();
        let deposit_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(1000, 2)),
        };
        engine.process_transaction(&deposit_tx).unwrap();
        let dispute_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        };
        engine.process_transaction(&dispute_tx).unwrap();
        let account = engine.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(0, 2));
        assert_eq!(account.held, Decimal::new(1000, 2));
        assert_eq!(account.total, Decimal::new(1000, 2));
        assert!(!account.locked);
        let resolve_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Resolve,
            client: 1,
            tx: 1,
            amount: None,
        };
        engine.process_transaction(&resolve_tx).unwrap();
        let account = engine.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(1000, 2));
        assert_eq!(account.held, Decimal::new(0, 2));
        assert_eq!(account.total, Decimal::new(1000, 2));
        assert!(!account.locked);
    }

    #[test]
    fn test_chargeback() {
        let mut engine = PaymentsEngine::new();
        let deposit_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(1000, 2)),
        };
        engine.process_transaction(&deposit_tx).unwrap();
        let dispute_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        };
        engine.process_transaction(&dispute_tx).unwrap();
        let chargeback_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Chargeback,
            client: 1,
            tx: 1,
            amount: None,
        };
        engine.process_transaction(&chargeback_tx).unwrap();
        let account = engine.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(0, 2));
        assert_eq!(account.held, Decimal::new(0, 2));
        assert_eq!(account.total, Decimal::new(0, 2));
        assert!(account.locked);
    }

    #[test]
    fn test_insufficient_funds() {
        let mut engine = PaymentsEngine::new();
        
        let withdrawal_tx = transaction::Transaction {
            tx_type: transaction::TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(500, 2)),
        };
        let result = engine.process_transaction(&withdrawal_tx);
        assert!(result.is_err());
        assert!(matches!(result, Err(PaymentsError::InsufficientFunds)));
    }
}