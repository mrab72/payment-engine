use std::io::{BufReader, Read};

use crate::errors::PaymentsError;
use crate::transaction::Transaction;

pub mod bounded;
pub mod concurrent;
pub mod standard;
pub mod concurrent_multi_engine;

use bounded::BoundedEngine;
use concurrent::ConcurrentEngine;
use standard::StandardEngine;
use concurrent_multi_engine::ConcurrentEngineV2;

/// Configuration for creating different types of payment engines
#[derive(Debug, Clone)]
pub enum EngineConfig {
    /// Standard payment engine with unlimited memory usage
    Standard,
    /// Memory-bounded engine with LRU eviction
    Bounded {
        max_accounts: usize,
        max_disputable_transactions: usize,
        max_processed_tx_ids: usize,
    },
    /// Concurrent engine for handling multiple streams
    Concurrent,

    /// Concurrent engine with multiple worker engines for true parallelism
    ConcurrentMultiEngine {
        num_workers: usize,
    },
}

impl EngineConfig {
    /// Create a standard configuration for small to medium datasets
    pub fn standard() -> Self {
        Self::Standard
    }

    /// Create a bounded configuration suitable for large datasets
    pub fn bounded(
        max_accounts: usize,
        max_disputable_transactions: usize,
        max_processed_tx_ids: usize,
    ) -> Self {
        Self::Bounded {
            max_accounts,
            max_disputable_transactions,
            max_processed_tx_ids,
        }
    }

    /// Create a concurrent configuration for high-throughput server environments
    pub fn concurrent() -> Self {
        Self::Concurrent
    }

    /// Create a concurrent multi-engine configuration for true parallelism
    pub fn concurrent_multi_engine(num_workers: usize) -> Self {
        Self::ConcurrentMultiEngine {
            num_workers,
        }
    }

    /// Create a bounded configuration optimized for the given available memory in MB
    /// Rough estimates: Account ~200 bytes, Transaction ~100 bytes, TxId ~4 bytes
    /// Accounts: 25%, Transactions: 50%, TxIds: 25%
    pub fn for_memory_mb(available_memory_mb: usize) -> Self {
        let account_memory_mb = available_memory_mb / 4;
        let transaction_memory_mb = available_memory_mb / 2;
        let tx_id_memory_mb = available_memory_mb / 4;

        let max_accounts = (account_memory_mb * 1024 * 1024) / 200;
        let max_transactions = (transaction_memory_mb * 1024 * 1024) / 100;
        let max_tx_ids = (tx_id_memory_mb * 1024 * 1024) / 4;

        Self::bounded(max_accounts, max_transactions, max_tx_ids)
    }

    /// Create engine config from optional CLI parameters
    /// This encapsulates the logic for handling optional engine type and custom limits
    /// max_accounts: default 10_000
    /// max_transactions: default 50_000
    /// max_tx_ids: default 1_000_000
    /// memory_limit_mb: default 100
    pub fn from_cli_params(
        engine_type: Option<&str>,
        max_accounts: Option<usize>,
        max_transactions: Option<usize>,
        max_tx_ids: Option<usize>,
        memory_limit_mb: Option<usize>,
    ) -> Self {
        // If memory limit is specified, use it to auto-configure bounded engine
        if let Some(memory_mb) = memory_limit_mb {
            return Self::for_memory_mb(memory_mb);
        }

        let engine_type = engine_type.unwrap_or("standard").to_lowercase();

        let max_accounts = max_accounts.unwrap_or(10_000);
        let max_transactions = max_transactions.unwrap_or(50_000);
        let max_tx_ids = max_tx_ids.unwrap_or(1_000_000);

        match engine_type.as_str() {
            "standard" => Self::standard(),
            "bounded" => Self::bounded(max_accounts, max_transactions, max_tx_ids),
            "concurrent" => Self::concurrent(),
            "concurrentmultiengine" | "concurrent_multi_engine" => {
                // For multi-engine, default to 4 workers if not specified
                let num_workers = 4;
                Self::concurrent_multi_engine(
                    num_workers,
                )
            }
            _ => {
                log::warn!(
                    "Unknown engine type: {}, defaulting to standard",
                    engine_type
                );
                Self::standard()
            }
        }
    }
}

/// Information about the engine's current state and capabilities
#[derive(Debug, Clone)]
pub struct EngineInfo {
    pub engine_type: String,
    pub memory_bounded: bool,
    pub concurrent: bool,
    pub account_count: usize,
    pub transaction_count: Option<usize>,
    pub memory_limits: Option<MemoryLimits>,
}

#[derive(Debug, Clone)]
pub struct MemoryLimits {
    pub max_accounts: usize,
    pub max_disputable_transactions: usize,
    pub max_processed_tx_ids: usize,
}

/// Unified payment engine that wraps different engine implementations
/// Users can choose the engine type based on their requirements
#[derive(Debug)]
pub enum PaymentsEngine {
    /// Standard engine for small to medium datasets
    Standard(StandardEngine),
    /// Memory-bounded engine for large datasets
    Bounded(BoundedEngine),
    /// Concurrent engine for high-throughput scenarios
    Concurrent(ConcurrentEngine),

    /// Concurrent multi-engine for true parallelism
    ConcurrentMultiEngine(ConcurrentEngineV2),
}

impl PaymentsEngine {
    /// Create a new payment engine with the specified configuration
    pub fn new(config: EngineConfig) -> Self {
        match config {
            EngineConfig::Standard => Self::Standard(StandardEngine::new()),
            EngineConfig::Bounded {
                max_accounts,
                max_disputable_transactions,
                max_processed_tx_ids,
            } => Self::Bounded(BoundedEngine::new(
                max_accounts,
                max_disputable_transactions,
                max_processed_tx_ids,
            )),
            EngineConfig::Concurrent {
            } => Self::Concurrent(ConcurrentEngine::new()),
            EngineConfig::ConcurrentMultiEngine {
                num_workers,
            } => Self::ConcurrentMultiEngine(ConcurrentEngineV2::new(num_workers)),
        }
    }

    /// Process a single transaction
    pub fn process_transaction(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        match self {
            Self::Standard(engine) => engine.process_transaction(transaction),
            Self::Bounded(engine) => engine.process_transaction(transaction),
            Self::Concurrent(engine) => engine.process_transaction(transaction),
            Self::ConcurrentMultiEngine(engine) => engine.process_transaction(transaction),
        }
    }

    /// Process transactions from any reader (file, network stream, etc.)
    pub fn process_transactions_from_reader<R: Read>(
        &mut self,
        reader: R,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Standard(engine) => engine.process_transactions_from_reader(reader),
            Self::Bounded(engine) => engine.process_transactions_from_reader(reader),
            Self::Concurrent(engine) => engine.process_transactions_from_reader(reader),
            Self::ConcurrentMultiEngine(engine) => engine.process_transactions_from_reader(reader),
        }
    }

    /// Process transactions from a CSV file
    pub fn process_transactions_from_file(
        &mut self,
        file_path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file = std::fs::File::open(file_path)?;
        let reader = BufReader::new(file);
        self.process_transactions_from_reader(reader)
    }

    /// Write current account states to CSV format
    pub fn write_accounts_csv<W: std::io::Write>(
        &self,
        writer: W,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Standard(engine) => engine.write_accounts_csv(writer),
            Self::Bounded(engine) => engine.write_accounts_csv(writer),
            Self::Concurrent(engine) => engine.write_accounts_csv(writer),
            Self::ConcurrentMultiEngine(engine) => engine.write_accounts_csv(writer),
        }
    }

    /// Get engine-specific information
    pub fn get_engine_info(&self) -> EngineInfo {
        match self {
            Self::Standard(engine) => engine.get_engine_info(),
            Self::Bounded(engine) => engine.get_engine_info(),
            Self::Concurrent(engine) => engine.get_engine_info(),
            Self::ConcurrentMultiEngine(engine) => engine.get_engine_info(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{Transaction, TransactionType};
    use rust_decimal::Decimal;

    #[test]
    fn test_standard_engine() {
        let mut engine = PaymentsEngine::new(EngineConfig::standard());
        let tx = Transaction {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(1000, 2)), // 10.00
        };
        engine.process_transaction(&tx).unwrap();
        let accounts = engine.get_engine_info().account_count;
        assert_eq!(accounts, 1);
    }

    #[test]
    fn test_bounded_engine() {
        let mut engine = PaymentsEngine::new(EngineConfig::bounded(100, 100, 1000));
        let tx = Transaction {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(1000, 2)),
        };
        engine.process_transaction(&tx).unwrap();
        let info = engine.get_engine_info();
        assert_eq!(info.engine_type, "Bounded");
        assert!(info.memory_bounded);
        assert!(!info.concurrent);
    }

    #[test]
    fn test_concurrent_engine() {
        let engine = PaymentsEngine::new(EngineConfig::concurrent());
        let info = engine.get_engine_info();
        assert_eq!(info.engine_type, "Concurrent");
        assert!(info.memory_bounded);
        assert!(info.concurrent);
    }

    #[test]
    fn test_memory_config() {
        let config = EngineConfig::for_memory_mb(100); // 100MB
        let engine = PaymentsEngine::new(config);
        let info = engine.get_engine_info();
        assert!(info.memory_limits.is_some());
    }
}
