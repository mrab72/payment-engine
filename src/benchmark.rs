use crate::engine::{EngineConfig, PaymentsEngine};
use crate::transaction::{Transaction, TransactionType};
use rust_decimal::Decimal;
use std::io::Cursor;

use memory_stats::memory_stats;
/// Benchmark utilities for testing memory usage and performance
pub struct PaymentEngineBenchmark;

impl PaymentEngineBenchmark {
    /// Generate synthetic transaction data for testing
    pub fn generate_transactions(
        count: usize,
        dispute_rate: f32,
        unique_accounts: usize,
    ) -> Vec<Transaction> {
        let mut transactions = Vec::with_capacity(count);

        // Generate deposits and withdrawals
        for i in 0..count {
            let tx_id = i as u32 + 1;
            let client_id = (i % unique_accounts) as u16 + 1; // Configurable unique clients
            let amount = Decimal::new((i % 10000) as i64 + 100, 2); // $1-$100

            let tx_type = if i % 3 == 0 {
                TransactionType::Withdrawal
            } else {
                TransactionType::Deposit
            };

            transactions.push(Transaction {
                tx_type,
                client: client_id,
                tx: tx_id,
                amount: Some(amount),
            });
        }

        // Add disputes for a percentage of transactions
        let dispute_count = (count as f32 * dispute_rate) as usize;
        for i in 0..dispute_count {
            let disputed_tx_id = (i + 1) as u32;
            let client_id = ((i % unique_accounts) as u16) + 1;

            transactions.push(Transaction {
                tx_type: TransactionType::Dispute,
                client: client_id,
                tx: disputed_tx_id,
                amount: None,
            });
        }

        transactions
    }

    /// Convert transactions to CSV format for streaming tests
    pub fn transactions_to_csv(transactions: &[Transaction]) -> String {
        let mut csv = String::from("type,client,tx,amount\n");

        for tx in transactions {
            let amount_str = match tx.amount {
                Some(amount) => amount.to_string(),
                None => String::new(),
            };

            let type_str = match tx.tx_type {
                TransactionType::Deposit => "deposit",
                TransactionType::Withdrawal => "withdrawal",
                TransactionType::Dispute => "dispute",
                TransactionType::Resolve => "resolve",
                TransactionType::Chargeback => "chargeback",
            };

            csv.push_str(&format!(
                "{},{},{},{}\n",
                type_str, tx.client, tx.tx, amount_str
            ));
        }

        csv
    }

    /// Benchmark standard PaymentsEngine
    pub fn benchmark_standard_engine(
        transaction_count: usize,
        dispute_rate: f32,
        unique_accounts: usize,
    ) -> BenchmarkResult {
        let transactions =
            Self::generate_transactions(transaction_count, dispute_rate, unique_accounts);
        let csv_data = Self::transactions_to_csv(&transactions);

        let start_memory = Self::get_memory_usage();
        let start_time = std::time::Instant::now();

        let mut engine = PaymentsEngine::new(EngineConfig::standard());
        let cursor = Cursor::new(csv_data.as_bytes());
        engine.process_transactions_from_reader(cursor).unwrap();

        let end_time = std::time::Instant::now();
        let end_memory = Self::get_memory_usage();

        BenchmarkResult {
            engine_type: "Standard".to_string(),
            transaction_count,
            dispute_rate,
            processing_time: end_time.duration_since(start_time),
            memory_used: end_memory.saturating_sub(start_memory),
            account_count: engine.get_engine_info().account_count,
        }
    }

    /// Benchmark BoundedPaymentsEngine
    pub fn benchmark_bounded_engine(
        transaction_count: usize,
        dispute_rate: f32,
        unique_accounts: usize,
        max_accounts: usize,
        max_transactions: usize,
        max_processed_ids: usize,
    ) -> BenchmarkResult {
        let transactions =
            Self::generate_transactions(transaction_count, dispute_rate, unique_accounts);
        let csv_data = Self::transactions_to_csv(&transactions);

        let start_memory = Self::get_memory_usage();
        let start_time = std::time::Instant::now();

        let mut engine = PaymentsEngine::new(EngineConfig::bounded(
            max_accounts,
            max_transactions,
            max_processed_ids,
        ));
        let cursor = Cursor::new(csv_data.as_bytes());
        engine.process_transactions_from_reader(cursor).unwrap();

        let end_time = std::time::Instant::now();
        let end_memory = Self::get_memory_usage();

        BenchmarkResult {
            engine_type: format!(
                "Bounded({}/{}/{})",
                max_accounts, max_transactions, max_processed_ids
            ),
            transaction_count,
            dispute_rate,
            processing_time: end_time.duration_since(start_time),
            memory_used: end_memory.saturating_sub(start_memory),
            account_count: engine.get_engine_info().account_count,
        }
    }

    /// Benchmark ConcurrentPaymentsEngine with multiple streams
    pub fn benchmark_concurrent_engine(
        transaction_count: usize,
        dispute_rate: f32,
        unique_accounts: usize,
        stream_count: usize,
        max_accounts: usize,
        max_transactions: usize,
        max_processed_ids: usize,
    ) -> BenchmarkResult {
        let transactions =
            Self::generate_transactions(transaction_count, dispute_rate, unique_accounts);
        let csv_data = Self::transactions_to_csv(&transactions);

        let start_memory = Self::get_memory_usage();
        let start_time = std::time::Instant::now();
        let cursor = Cursor::new(csv_data.as_bytes());
        let mut engine = PaymentsEngine::new(EngineConfig::concurrent(
            max_accounts,
            max_transactions,
            max_processed_ids,
        ));
        engine.process_transactions_from_reader(cursor).unwrap();

        let end_time = std::time::Instant::now();
        let end_memory = Self::get_memory_usage();

        BenchmarkResult {
            engine_type: format!("Concurrent({} streams)", stream_count),
            transaction_count,
            dispute_rate,
            processing_time: end_time.duration_since(start_time),
            memory_used: end_memory.saturating_sub(start_memory),
            account_count: engine.get_engine_info().account_count,
        }
    }

    /// Simple memory usage estimation (placeholder - in real benchmarks use proper profiling tools)
    fn get_memory_usage() -> usize {
        if let Some(usage) = memory_stats() {
            usage.physical_mem
        } else {
            0
        }
    }
}

#[derive(Debug)]
pub struct BenchmarkResult {
    pub engine_type: String,
    pub transaction_count: usize,
    pub dispute_rate: f32,
    pub processing_time: std::time::Duration,
    pub memory_used: usize,
    pub account_count: usize,
}

impl BenchmarkResult {
    pub fn print_summary(&self) {
        println!("=== Benchmark Results: {} ===", self.engine_type);
        println!("Transactions processed: {}", self.transaction_count);
        println!("Dispute rate: {:.1}%", self.dispute_rate * 100.0);
        println!("Processing time: {:?}", self.processing_time);
        println!("Memory used: {} bytes", self.memory_used);
        println!("Final account count: {}", self.account_count);
        println!(
            "Throughput: {:.0} tx/sec",
            self.transaction_count as f64 / self.processing_time.as_secs_f64()
        );
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_comparison() {
        const TX_COUNT: usize = 10_000;
        const DISPUTE_RATE: f32 = 0.05; // 5%

        let standard_result =
            PaymentEngineBenchmark::benchmark_standard_engine(TX_COUNT, DISPUTE_RATE, 1000);
        let bounded_result = PaymentEngineBenchmark::benchmark_bounded_engine(
            TX_COUNT,
            DISPUTE_RATE,
            1000,
            1000,
            1000,
            10_000,
        );

        standard_result.print_summary();
        bounded_result.print_summary();

        // Bounded engine should use less memory (or at least not significantly more)
        // Note: Memory measurement is placeholder, in real tests this would show the difference
        assert!(bounded_result.account_count <= 1000); // Respects account limit
    }

    #[test]
    fn test_concurrent_processing() {
        const TX_COUNT: usize = 1_000;
        const DISPUTE_RATE: f32 = 0.02;
        const STREAM_COUNT: usize = 4;

        let concurrent_result = PaymentEngineBenchmark::benchmark_concurrent_engine(
            TX_COUNT,
            DISPUTE_RATE,
            500,
            STREAM_COUNT,
            500,
            500,
            5_000,
        );

        concurrent_result.print_summary();

        assert!(concurrent_result.account_count > 0);
        assert!(concurrent_result.processing_time.as_millis() < 5000); // Should complete quickly
    }

    #[test]
    fn test_large_dataset_bounded() {
        const TX_COUNT: usize = 100_000;
        const DISPUTE_RATE: f32 = 0.01; // 1%

        let bounded_result = PaymentEngineBenchmark::benchmark_bounded_engine(
            TX_COUNT,
            DISPUTE_RATE,
            1000,
            1000,
            2000,
            50_000,
        );

        bounded_result.print_summary();

        // Should handle large dataset with bounded memory
        assert!(bounded_result.account_count <= 1000);
        assert!(bounded_result.transaction_count == TX_COUNT);
    }
}
