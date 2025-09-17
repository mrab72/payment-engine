use std::io::Read;
use std::sync::{Arc, Mutex};

use crate::libs::{
    PaymentsError,
    transaction::Transaction,
    account::Account,
};
use super::{EngineInfo, MemoryLimits, bounded::BoundedEngine};

/// Concurrent TCP stream processing engine for handling thousands of concurrent streams.
/// Uses thread-safe Arc<Mutex<BoundedEngine>> for shared state management.
/// Each stream processes transactions independently while maintaining global consistency.
#[derive(Debug)]
pub struct ConcurrentEngine {
    engine: Arc<Mutex<BoundedEngine>>,
    memory_limits: MemoryLimits,
}

impl ConcurrentEngine {
    pub fn new(
        max_accounts: usize,
        max_disputable_transactions: usize,
        max_processed_tx_ids: usize,
    ) -> Self {
        let engine = BoundedEngine::new(
            max_accounts,
            max_disputable_transactions,
            max_processed_tx_ids,
        );
        let memory_limits = MemoryLimits {
            max_accounts,
            max_disputable_transactions,
            max_processed_tx_ids,
        };
        Self {
            engine: Arc::new(Mutex::new(engine)),
            memory_limits,
        }
    }

    pub fn process_transaction(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        let mut engine_guard = self.engine.lock().map_err(|e| {
            PaymentsError::InvalidTransaction(format!("Failed to acquire engine lock: {}", e))
        })?;
        engine_guard.process_transaction(transaction)
    }

    /// Process transactions from a single TCP stream.
    /// This method can be called concurrently from multiple threads/tasks.
    /// Each stream is processed independently with minimal lock contention.
    pub fn process_stream_transactions<R: Read + Send + 'static>(
        &self,
        reader: R,
        stream_id: u64,
    ) -> std::thread::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
        let engine = self.engine.clone();
        
        std::thread::spawn(move || {
            let mut rdr = csv::ReaderBuilder::new()
                .trim(csv::Trim::All)
                .from_reader(reader);

            log::debug!("Processing transactions from stream {}", stream_id);

            for (idx, line) in rdr.deserialize().enumerate() {
                let transaction: Transaction = match line {
                    Ok(tx) => tx,
                    Err(e) => {
                        log::error!("Stream {}: Failed to parse line {}: {}", stream_id, idx + 1, e);
                        continue;
                    }
                };

                // Acquire lock only for the duration of transaction processing
                let result = {
                    let mut engine_guard = engine.lock().map_err(|e| {
                        format!("Failed to acquire engine lock: {}", e)
                    })?;
                    engine_guard.process_transaction(&transaction)
                };

                if let Err(e) = result {
                    log::error!("Stream {}: Failed to process transaction {:?}: {}", stream_id, transaction, e);
                } else {
                    log::debug!("Stream {}: Successfully processed transaction: {:?}", stream_id, transaction);
                }
            }

            log::info!("Completed processing stream {}", stream_id);
            Ok(())
        })
    }

    /// Process multiple concurrent streams and wait for all to complete.
    /// Returns when all streams have finished processing.
    pub fn process_concurrent_streams<R: Read + Send + 'static>(
        &self,
        streams: Vec<R>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let handles: Vec<_> = streams
            .into_iter()
            .enumerate()
            .map(|(idx, reader)| {
                self.process_stream_transactions(reader, idx as u64)
            })
            .collect();

        // Wait for all streams to complete
        for (idx, handle) in handles.into_iter().enumerate() {
            match handle.join() {
                Ok(Ok(())) => log::info!("Stream {} completed successfully", idx),
                Ok(Err(e)) => log::error!("Stream {} failed: {}", idx, e),
                Err(e) => log::error!("Stream {} panicked: {:?}", idx, e),
            }
        }

        Ok(())
    }

    pub fn process_transactions_from_reader<R: Read>(&mut self, reader: R) -> Result<(), Box<dyn std::error::Error>> {
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(reader);

        log::debug!("Starting to process transactions from stream (concurrent engine)");

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
        let engine = self.engine.lock().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to acquire engine lock for export: {}", e))
        })?;
        engine.write_accounts_csv(writer)
    }

    pub fn get_accounts(&self) -> Result<Vec<Account>, Box<dyn std::error::Error>> {
        let engine = self.engine.lock().map_err(|e| {
            format!("Failed to acquire engine lock: {}", e)
        })?;
        Ok(engine.get_accounts())
    }

    pub fn get_engine_info(&self) -> EngineInfo {
        if let Ok(engine) = self.engine.lock() {
            EngineInfo {
                engine_type: "Concurrent".to_string(),
                memory_bounded: true,
                concurrent: true,
                account_count: engine.get_accounts().len(),
                transaction_count: None, // Can't easily determine due to concurrency
                memory_limits: Some(self.memory_limits.clone()),
            }
        } else {
            EngineInfo {
                engine_type: "Concurrent (locked)".to_string(),
                memory_bounded: true,
                concurrent: true,
                account_count: 0,
                transaction_count: None,
                memory_limits: Some(self.memory_limits.clone()),
            }
        }
    }
}
