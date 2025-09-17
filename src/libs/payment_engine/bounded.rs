use std::io::Read;
use std::num::NonZeroUsize;
use lru::LruCache;
use rust_decimal::Decimal;

use crate::libs::{
    PaymentsError,
    transaction::{Transaction, TxId, StoredTransaction, TransactionType},
    account::{Account, ClientId},
};
use super::{EngineInfo, MemoryLimits};

/// Memory-bounded payment engine for handling extremely large datasets.
/// Uses LRU caches to limit memory usage while still providing correct processing.
#[derive(Debug)]
pub struct BoundedEngine {
    /// LRU cache of active accounts, evicts least recently used accounts
    accounts: LruCache<ClientId, Account>,
    
    /// LRU cache of disputable transactions
    disputable_transactions: LruCache<TxId, StoredTransaction>,
    
    /// LRU cache of processed transaction IDs for duplicate prevention
    processed_tx_ids: LruCache<TxId, ()>,

    /// Store memory limits for reporting
    memory_limits: MemoryLimits,
}

impl BoundedEngine {
    pub fn new(
        max_accounts: usize,
        max_disputable_transactions: usize, 
        max_processed_tx_ids: usize,
    ) -> Self {
        Self {
            accounts: LruCache::new(NonZeroUsize::new(max_accounts).unwrap()),
            disputable_transactions: LruCache::new(NonZeroUsize::new(max_disputable_transactions).unwrap()),
            processed_tx_ids: LruCache::new(NonZeroUsize::new(max_processed_tx_ids).unwrap()),
            memory_limits: MemoryLimits {
                max_accounts,
                max_disputable_transactions,
                max_processed_tx_ids,
            },
        }
    }

    /// Retrieves an existing account or creates a new one if it doesn't exist.
    /// May evict least recently used account if cache is full.
    fn get_or_create_account(&mut self, client_id: ClientId) -> &mut Account {
        if !self.accounts.contains(&client_id) {
            self.accounts.put(client_id, Account::new(client_id));
        }
        self.accounts.get_mut(&client_id).unwrap()
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
        self.disputable_transactions.put(transaction.tx, StoredTransaction {
            client: client_id,
            amount,
            disputed: false,
        });
        
        // Track transaction ID for duplicate prevention
        self.processed_tx_ids.put(transaction.tx, ());

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
        self.disputable_transactions.put(transaction.tx, StoredTransaction {
            client: client_id,
            amount,
            disputed: false,
        });
        
        // Track transaction ID for duplicate prevention
        self.processed_tx_ids.put(transaction.tx, ());
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
        self.disputable_transactions.pop(&transaction.tx);
        
        Ok(())
    }

    pub fn process_transactions_from_reader<R: Read>(&mut self, reader: R) -> Result<(), Box<dyn std::error::Error>> {
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(reader);

        log::debug!("Starting to process transactions from stream (bounded engine)");

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
        
        for (_, account) in self.accounts.iter() {
            wtr.serialize(account)?;
        }

        wtr.flush()?;
        log::info!("Successfully wrote accounts to CSV (bounded engine)");
        Ok(())
    }

    pub fn get_accounts(&self) -> Vec<Account> {
        self.accounts.iter().map(|(_, account)| account.clone()).collect()
    }

    pub fn get_engine_info(&self) -> EngineInfo {
        EngineInfo {
            engine_type: "Bounded".to_string(),
            memory_bounded: true,
            concurrent: false,
            account_count: self.accounts.len(),
            transaction_count: Some(self.disputable_transactions.len()),
            memory_limits: Some(self.memory_limits.clone()),
        }
    }
}
