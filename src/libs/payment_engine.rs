use std::collections::HashMap;
use rust_decimal::Decimal;
use serde::Deserialize;

use super::account::{Account, ClientId};
use super::transaction::{TxId, StoredTransaction};

#[derive(Debug, Clone, Deserialize)]
pub struct PaymentsEngine {
    accounts: HashMap<ClientId, Account>,
    transactions: HashMap<TxId, StoredTransaction>,
}


impl Default for PaymentsEngine {
    fn default() -> Self {
        Self {
            accounts: HashMap::new(),
            transactions: HashMap::new(),
        }
    }
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
        let amount = transaction.amount.ok_or_else(|| super::PaymentsError::TransactionNotFound)?;
        if amount <= Decimal::ZERO {
            return Err(super::PaymentsError::InvalidTransaction("Withdrawal amount must be positive".to_string()));
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
        if transaction.amount.is_none() {
            return Err(super::PaymentsError::TransactionNotFound);
        }

        let amount = transaction.amount.ok_or_else(|| super::PaymentsError::TransactionNotFound)?;
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

        wtr.write_record(&["client", "available", "held", "total", "locked"])?;
        
        for account in self.accounts.values() {
            wtr.serialize(account)?;
        }
 
        wtr.flush()?;
        log::info!("Successfully wrote accounts to CSV");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    
}