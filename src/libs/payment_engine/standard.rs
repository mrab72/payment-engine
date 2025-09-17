use std::collections::{HashMap, HashSet};
use std::io::Read;
use rust_decimal::Decimal;

use crate::libs::{
    PaymentsError,
    transaction::{Transaction, TxId, StoredTransaction, TransactionType},
    account::{Account, ClientId},
};
use super::EngineInfo;

/// Standard payment engine with unlimited memory usage.
/// Suitable for small to medium datasets where memory is not a constraint.
#[derive(Debug, Clone, Default)]
pub struct StandardEngine {
    /// Mapping of client IDs to their accounts.
    accounts: HashMap<ClientId, Account>,

    /// Record of disputable transactions (deposits/withdrawals) keyed by transaction ID.
    /// Only stores transactions that can potentially be disputed.
    disputable_transactions: HashMap<TxId, StoredTransaction>,

    /// Set of all processed transaction IDs to prevent duplicates.
    processed_tx_ids: HashSet<TxId>,
}

impl StandardEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retrieves an existing account or creates a new one if it doesn't exist.
    fn get_or_create_account(&mut self, client_id: ClientId) -> &mut Account {
        self.accounts.entry(client_id).or_insert_with(|| Account::new(client_id))
    }

    pub fn process_transaction(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        match transaction.tx_type {
            TransactionType::Deposit => self.process_deposit(transaction),
            TransactionType::Withdrawal => self.process_withdrawal(transaction),
            TransactionType::Dispute => self.process_dispute(transaction),
            TransactionType::Resolve => self.process_resolve(transaction),
            TransactionType::Chargeback => self.process_chargeback(transaction),
        }
    }

    fn process_deposit(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        let amount = transaction.amount.ok_or(PaymentsError::InvalidTransaction("Deposit transaction must have an amount".to_string()))?;
        if amount <= Decimal::ZERO {
            return Err(PaymentsError::InvalidTransaction("Deposit amount must be positive".to_string()));
        }
        if self.processed_tx_ids.contains(&transaction.tx) {
            return Err(PaymentsError::InvalidTransaction(format!("Transaction ID {} already exists", transaction.tx)));
        }
        let client_id = transaction.client;
        let account = self.get_or_create_account(client_id);
        account.deposit(amount)?;

        // Store disputable transaction for potential future disputes
        self.disputable_transactions.insert(transaction.tx, StoredTransaction {
            client: client_id,
            amount,
            disputed: false,
        });
        
        // Track transaction ID for duplicate prevention
        self.processed_tx_ids.insert(transaction.tx);

        Ok(())
    }

    fn process_withdrawal(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        let amount = transaction.amount.ok_or(PaymentsError::InvalidTransaction("Withdrawal transaction must have an amount".to_string()))?;
        if amount <= Decimal::ZERO {
            return Err(PaymentsError::InvalidTransaction("Withdrawal amount must be positive".to_string()));
        }
        if self.processed_tx_ids.contains(&transaction.tx) {
            return Err(PaymentsError::InvalidTransaction(format!("Transaction ID {} already exists", transaction.tx)));
        }
        let client_id = transaction.client;
        let account = self.get_or_create_account(client_id);
        account.withdraw(amount)?;
        
        // Store disputable transaction for potential future disputes
        self.disputable_transactions.insert(transaction.tx, StoredTransaction {
            client: client_id,
            amount,
            disputed: false,
        });
        
        // Track transaction ID for duplicate prevention
        self.processed_tx_ids.insert(transaction.tx);
        Ok(())
    }

    fn process_dispute(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        if transaction.amount.is_some() {
            return Err(PaymentsError::InvalidTransaction("Dispute transaction should not have an amount".to_string()));
        }

        let (client_id, amount) = {
            let stored_tx = self
                .disputable_transactions
                .get_mut(&transaction.tx)
                .ok_or(PaymentsError::TransactionNotFound)?;

            if stored_tx.client != transaction.client {
                return Err(PaymentsError::ClientIdMismatch);
            }

            if stored_tx.disputed {
                return Err(PaymentsError::TransactionAlreadyDisputed(transaction.tx));
            }

            stored_tx.disputed = true;

            (stored_tx.client, stored_tx.amount)
        };

        let account = self.get_or_create_account(client_id);
        account.hold(amount)?;
        Ok(())
    }

    fn process_resolve(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        if transaction.amount.is_some() {
            return Err(PaymentsError::InvalidTransaction("Resolve transaction should not have an amount".to_string()));
        }
        let (client_id, amount) = {
            let stored_tx = self.disputable_transactions.get_mut(&transaction.tx).ok_or(PaymentsError::TransactionNotFound)?;
            if stored_tx.client != transaction.client {
                return Err(PaymentsError::ClientIdMismatch);
            }

            if !stored_tx.disputed {
                return Err(PaymentsError::TransactionNotDisputed);
            }
            stored_tx.disputed = false;
            (stored_tx.client, stored_tx.amount)
        };

        let account = self.get_or_create_account(client_id);
        account.release(amount)?;
        
        Ok(())
    }

    fn process_chargeback(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        if transaction.amount.is_some() {
            return Err(PaymentsError::InvalidTransaction("Chargeback transaction should not have an amount".to_string()));
        }
        let (client_id, amount) = {
            let stored_tx = self.disputable_transactions.get_mut(&transaction.tx).ok_or(PaymentsError::TransactionNotFound)?;
            if stored_tx.client != transaction.client {
                return Err(PaymentsError::ClientIdMismatch);
            }

            if !stored_tx.disputed {
                return Err(PaymentsError::TransactionNotDisputed);
            }
            stored_tx.disputed = false;
            (stored_tx.client, stored_tx.amount)
        };

        let account = self.get_or_create_account(client_id);
        account.chargeback(amount)?;
        
        // After chargeback, we can remove the transaction since it's finalized
        self.disputable_transactions.remove(&transaction.tx);
        
        Ok(())
    }

    pub fn process_transactions_from_reader<R: Read>(&mut self, reader: R) -> Result<(), Box<dyn std::error::Error>> {
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(reader);

        log::debug!("Starting to process transactions from stream (standard engine)");

        for (idx, line) in rdr.deserialize().enumerate() {
            let transaction: Transaction = match line {
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

    pub fn write_accounts_csv<W: std::io::Write>(&self, writer: W) -> Result<(), Box<dyn std::error::Error>> {
        let mut wtr = csv::WriterBuilder::new()
            .has_headers(true)
            .from_writer(writer);

        wtr.write_record(["client", "available", "held", "total", "locked"])?;
        
        for account in self.accounts.values() {
            wtr.serialize(account)?;
        }

        wtr.flush()?;
        log::info!("Successfully wrote accounts to CSV (standard engine)");
        Ok(())
    }

    pub fn get_accounts(&self) -> Vec<Account> {
        self.accounts.values().cloned().collect()
    }

    pub fn get_engine_info(&self) -> EngineInfo {
        EngineInfo {
            engine_type: "Standard".to_string(),
            memory_bounded: false,
            concurrent: false,
            account_count: self.accounts.len(),
            transaction_count: Some(self.disputable_transactions.len()),
            memory_limits: None,
        }
    }
}
