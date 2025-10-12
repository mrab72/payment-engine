use std::io::Read;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread;
use dashmap::DashMap;

use crate::engine::standard::StandardEngine;
use crate::errors::PaymentsError;
use crate::transaction::Transaction;
use super::EngineInfo;

/// Concurrent engine with one BoundedEngine per worker for true parallelism
/// Uses DashMap for lock-free transaction ID checking
#[derive(Debug)]
pub struct ConcurrentEngineV2 {
    worker_engines: Vec<Arc<Mutex<StandardEngine>>>,
    global_tx_ids: Arc<DashMap<u32, u16>>,  // tx_id -> client_id
    num_workers: usize,
}

impl ConcurrentEngineV2 {
    pub fn new(num_workers: usize) -> Self {
        let mut worker_engines = Vec::with_capacity(num_workers);
        
        // Create one engine per worker
        for _ in 0..num_workers {
            let engine = StandardEngine::new();
            worker_engines.push(Arc::new(Mutex::new(engine)));
        }
        
        Self {
            worker_engines,
            global_tx_ids: Arc::new(DashMap::new()),
            num_workers,
        }
    }

    pub fn process_transaction(&mut self, transaction: &Transaction) -> Result<(), PaymentsError> {
        if let Some(existing_client) = self.global_tx_ids.get(&transaction.tx) {
            return Err(PaymentsError::InvalidTransaction(format!(
                "Transaction ID {} already exists for client {}",
                transaction.tx,
                *existing_client
            )));
        }

        // 2. Determine which worker handles this client
        let worker_id = (transaction.client as usize) % self.num_workers;
        
        // 3. Process in the worker's engine (only locks THIS worker's engine)
        let mut engine_guard = self.worker_engines[worker_id]
            .lock()
            .map_err(|e| {
                PaymentsError::InvalidTransaction(format!("Failed to acquire worker lock: {}", e))
            })?;
        
        engine_guard.process_transaction(transaction)?;
        
        // 4. Register transaction ID globally
        self.global_tx_ids.insert(transaction.tx, transaction.client);
        
        Ok(())
    }

    pub fn process_transactions_from_reader<R: Read>(
        &mut self,
        reader: R,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create channels for each worker
        let mut worker_senders = Vec::new();
        let mut worker_receivers = Vec::new();
        for _ in 0..self.num_workers {
            let (tx, rx) = mpsc::channel::<Transaction>();
            worker_senders.push(tx);
            worker_receivers.push(rx);
        }

        log::info!(
            "Starting concurrent processing with {} workers (one engine per worker)",
            self.num_workers
        );

        // Spawn worker threads
        let mut handles = Vec::new();
        for worker_id in 0..self.num_workers {
            let engine = self.worker_engines[worker_id].clone();
            let global_tx_ids = self.global_tx_ids.clone();
            let rx = worker_receivers.remove(0);

            let handle = thread::spawn(
                move || -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
                    let mut processed_count = 0;

                    while let Ok(transaction) = rx.recv() {
                        // Check global tx IDs (lock-free read)
                        if global_tx_ids.contains_key(&transaction.tx) {
                            log::error!(
                                "Worker {}: Duplicate transaction ID {}",
                                worker_id,
                                transaction.tx
                            );
                            continue;
                        }

                        // Process in worker's engine
                        let result = {
                            let mut engine_guard = engine.lock().map_err(|e| {
                                format!("Worker {}: Failed to acquire lock: {}", worker_id, e)
                            })?;
                            engine_guard.process_transaction(&transaction)
                        };

                        match result {
                            Ok(()) => {
                                // Register globally
                                global_tx_ids.insert(transaction.tx, transaction.client);
                                processed_count += 1;
                                log::debug!(
                                    "Worker {}: Processed tx:{} for client:{}",
                                    worker_id,
                                    transaction.tx,
                                    transaction.client
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

                    log::info!("Worker {} processed {} transactions", worker_id, processed_count);
                    Ok(processed_count)
                },
            );

            handles.push(handle);
        }

        // Read CSV and distribute to workers
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

            // Route to worker based on client ID
            let worker_id = (transaction.client as usize) % self.num_workers;
            
            if let Err(e) = worker_senders[worker_id].send(transaction) {
                log::error!("Failed to send to worker {}: {}", worker_id, e);
                break;
            }
            sent_count += 1;
        }

        // Signal completion
        drop(worker_senders);
        log::info!("Sent {} transactions to workers", sent_count);

        // Wait for workers
        let mut total_processed = 0;
        for (worker_id, handle) in handles.into_iter().enumerate() {
            match handle.join() {
                Ok(Ok(count)) => total_processed += count,
                Ok(Err(e)) => log::error!("Worker {} failed: {}", worker_id, e),
                Err(e) => log::error!("Worker {} panicked: {:?}", worker_id, e),
            }
        }

        log::info!("Total processed: {}", total_processed);
        Ok(())
    }

    pub fn write_accounts_csv<W: std::io::Write>(
        &self,
        writer: W,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut wtr = csv::Writer::from_writer(writer);
        wtr.write_record(&["client", "available", "held", "total", "locked"])?;

        // Collect accounts from all workers
        for (worker_id, engine_arc) in self.worker_engines.iter().enumerate() {
            let engine = engine_arc.lock().map_err(|e| {
                std::io::Error::other(format!("Failed to lock worker {}: {}", worker_id, e))
            })?;

            // Export accounts from this worker's engine
            for (client_id, account) in engine.accounts.iter() {
                wtr.write_record(&[
                    client_id.to_string(),
                    format!("{:.4}", account.available),
                    format!("{:.4}", account.held),
                    format!("{:.4}", account.total),
                    account.locked.to_string(),
                ])?;
            }
        }

        wtr.flush()?;
        Ok(())
    }

    pub fn get_engine_info(&self) -> EngineInfo {
        let mut total_accounts = 0;
        for engine_arc in &self.worker_engines {
            if let Ok(engine) = engine_arc.lock() {
                total_accounts += engine.accounts.len();
            }
        }

        EngineInfo {
            engine_type: "ConcurrentMultiEngine".to_string(),
            memory_bounded: false,
            concurrent: true,
            account_count: total_accounts,
            transaction_count: None, // Not tracked globally
            memory_limits: None,     // Not applicable
        }
    }
}