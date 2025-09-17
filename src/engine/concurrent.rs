use std::io::Read;
use std::sync::{Arc, Mutex};

use std::sync::mpsc;
use std::thread;

use super::{EngineInfo, MemoryLimits, bounded::BoundedEngine};
use crate::errors::PaymentsError;
use crate::transaction::Transaction;

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
                        log::error!(
                            "Stream {}: Failed to parse line {}: {}",
                            stream_id,
                            idx + 1,
                            e
                        );
                        continue;
                    }
                };

                // Acquire lock only for the duration of transaction processing
                let result = {
                    let mut engine_guard = engine
                        .lock()
                        .map_err(|e| format!("Failed to acquire engine lock: {}", e))?;
                    engine_guard.process_transaction(&transaction)
                };

                if let Err(e) = result {
                    log::error!(
                        "Stream {}: Failed to process transaction {:?}: {}",
                        stream_id,
                        transaction,
                        e
                    );
                } else {
                    log::debug!(
                        "Stream {}: Successfully processed transaction: {:?}",
                        stream_id,
                        transaction
                    );
                }
            }

            log::info!("Completed processing stream {}", stream_id);
            Ok(())
        })
    }

    // Process transactions from reader using concurrent worker threads
    /// This version batches transactions and distributes them across multiple threads
    pub fn process_transactions_from_reader<R: Read>(
        &mut self,
        reader: R,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let num_workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        let (tx, rx) = mpsc::channel::<Transaction>();
        let rx = Arc::new(Mutex::new(rx));

        log::debug!(
            "Starting concurrent transaction processing with {} workers",
            num_workers
        );

        let mut handles = Vec::new();
        for worker_id in 0..num_workers {
            let engine = self.engine.clone();
            let rx = rx.clone();

            let handle = thread::spawn(
                move || -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
                    let mut processed_count = 0;

                    loop {
                        let transaction = {
                            let rx_guard = rx.lock().map_err(|e| {
                                format!(
                                    "Worker {}: Failed to acquire receiver lock: {}",
                                    worker_id, e
                                )
                            })?;

                            match rx_guard.recv() {
                                Ok(tx) => tx,
                                Err(_) => break,
                            }
                        };

                        // Process the transaction
                        let result = {
                            let mut engine_guard = engine.lock().map_err(|e| {
                                format!(
                                    "Worker {}: Failed to acquire engine lock: {}",
                                    worker_id, e
                                )
                            })?;
                            engine_guard.process_transaction(&transaction)
                        };

                        match result {
                            Ok(()) => {
                                processed_count += 1;
                                log::debug!(
                                    "Worker {}: Successfully processed transaction: {:?}",
                                    worker_id,
                                    transaction
                                );
                            }
                            Err(e) => {
                                log::error!(
                                    "Worker {}: Failed to process transaction {:?}: {}",
                                    worker_id,
                                    transaction,
                                    e
                                );
                            }
                        }
                    }

                    log::info!(
                        "Worker {} completed, processed {} transactions",
                        worker_id,
                        processed_count
                    );
                    Ok(processed_count)
                },
            );

            handles.push(handle);
        }

        // Read and send transactions to workers
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(reader);

        let mut sent_count = 0;
        for (idx, line) in rdr.deserialize().enumerate() {
            let transaction: Transaction = match line {
                Ok(tx) => tx,
                Err(e) => {
                    log::error!("Failed to parse line {}: {}", idx + 1, e);
                    continue;
                }
            };

            if let Err(e) = tx.send(transaction) {
                log::error!("Failed to send transaction to workers: {}", e);
                break;
            }
            sent_count += 1;
        }

        // Close the channel to signal workers to stop
        drop(tx);

        log::info!("Sent {} transactions to workers", sent_count);

        // Wait for all workers to complete and collect results
        let mut total_processed = 0;
        for (worker_id, handle) in handles.into_iter().enumerate() {
            match handle.join() {
                Ok(Ok(processed)) => {
                    total_processed += processed;
                    log::info!(
                        "Worker {} completed successfully, processed {} transactions",
                        worker_id,
                        processed
                    );
                }
                Ok(Err(e)) => log::error!("Worker {} failed: {}", worker_id, e),
                Err(e) => log::error!("Worker {} panicked: {:?}", worker_id, e),
            }
        }

        log::info!(
            "All workers completed. Total processed: {}",
            total_processed
        );
        Ok(())
    }

    pub fn write_accounts_csv<W: std::io::Write>(
        &self,
        writer: W,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let engine = self.engine.lock().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to acquire engine lock for export: {}", e),
            )
        })?;
        engine.write_accounts_csv(writer)
    }

    pub fn get_engine_info(&self) -> EngineInfo {
        if let Ok(engine) = self.engine.lock() {
            EngineInfo {
                engine_type: "Concurrent".to_string(),
                memory_bounded: true,
                concurrent: true,
                account_count: engine.accounts.len(),
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
