# Payment Engine

A robust, high-performance payment processing engine written in Rust that handles various transaction types including deposits, withdrawals, disputes, resolutions, and chargebacks.

## Features

- **Transaction Processing**: Supports deposits, withdrawals, disputes, resolutions, and chargebacks
- **Account Management**: Automatic account creation and balance tracking
- **Safety & Security**: Comprehensive validation, error handling, and account locking mechanisms
- **CSV Input/Output**: Processes transactions from CSV files and outputs account states
- **Precision**: Uses `rust_decimal` for accurate financial calculations
- **Logging**: Configurable logging levels for debugging and monitoring
- **Pluggable Engines**: Choose between `standard`, `bounded` (LRU-capped memory), and `concurrent`
- **Memory Controls**: Set explicit caps or auto-size via `--memory-limit-mb`

## Installation

### Prerequisites

- Rust 1.70+ (2024 edition)
- Cargo package manager

### Building from Source

```bash
git clone <repository-url>
cd payment-engine
cargo build --release
```

## Usage

### Command Line Interface

```bash
# Basic usage - read from CSV file and output to stdout
./target/release/payments-engine transactions.csv

# Output to a specific file
./target/release/payments-engine transactions.csv --output accounts.csv

# With debug logging
./target/release/payments-engine transactions.csv --log-level debug

# Use a specific engine (standard | bounded | concurrent)
./target/release/payments-engine transactions.csv --engine bounded \
  --max-accounts 10000 --max-transactions 50000 --max-tx-ids 1000000

# Auto-size bounded engine for a memory budget (in MB)
./target/release/payments-engine transactions.csv --memory-limit-mb 256

```

### Command Line Options

- `<input_file>`: Path to the input CSV file containing transactions (required)
- `--output, -o <file>`: Output file path (optional, defaults to stdout)
- `--log-level, -l <level>`: Log level - error, warn, info, debug, trace (optional, defaults to info)
- `--engine, -e <type>`: Engine type: `standard` (default), `bounded`, or `concurrent` 
- `--max-accounts <n>`: Max accounts in memory (bounded/concurrent). Default: 10,000
- `--max-transactions <n>`: Max disputable transactions in memory (bounded/concurrent). Default: 50,000
- `--max-tx-ids <n>`: Max processed transaction IDs in memory (bounded/concurrent). Default: 1,000,000
- `--memory-limit-mb <n>`: Auto-configure bounded engine based on memory budget; overrides the three max-* options

### Input CSV Format

The input CSV file should have the following columns:

```csv
type,client,tx,amount
deposit,1,1,1.0
deposit,2,2,2.0
deposit,1,3,2.0
withdrawal,1,4,1.5
withdrawal,2,5,3.0
dispute,1,1,
resolve,1,1,
chargeback,1,3,
```

#### Column Descriptions

- **type**: Transaction type (`deposit`, `withdrawal`, `dispute`, `resolve`, `chargeback`)
- **client**: Client ID (16-bit unsigned integer)
- **tx**: Transaction ID (32-bit unsigned integer)
- **amount**: Transaction amount (decimal, required for deposit/withdrawal, empty for dispute/resolve/chargeback)

### Output CSV Format

The output contains account states with the following columns:

```csv
client,available,held,total,locked
1,1.5,0.0,1.5,false
2,2.0,0.0,2.0,false
```

#### Column Descriptions

- **client**: Client ID
- **available**: Available funds for transactions
- **held**: Funds held due to disputes
- **total**: Total funds (available + held)
- **locked**: Account lock status (true if locked due to chargeback)

## Transaction Types

### Deposit
- Adds funds to a client's account
- Increases both `available` and `total` balances
- Requires a positive amount
- Creates account if it doesn't exist

### Withdrawal
- Removes funds from a client's account
- Decreases both `available` and `total` balances
- Requires sufficient available funds
- Account must not be locked

### Dispute
- Places a hold on funds from a previous deposit
- Moves funds from `available` to `held`
- References an existing transaction by ID
- Client ID must match the original transaction
- Transaction must not already be disputed

### Resolve
- Releases a disputed transaction
- Moves funds from `held` back to `available`
- Transaction must be under dispute
- Client ID must match the original transaction

### Chargeback
- Reverses a disputed transaction
- Removes funds from `held` and decreases `total`
- Locks the account permanently
- Transaction must be under dispute
- Client ID must match the original transaction

## Architecture

### Core Components

- **PaymentsEngine**: Main facade that processes transactions and manages accounts
- **Account**: Represents a client account with balances and lock status
- **Transaction**: Input transaction structure
- **StoredTransaction**: Internal transaction record with dispute status

### Engine Variants

#### Standard Engine
**Design**: In-memory `HashMap`/`HashSet` with unlimited growth
- ✅ **Pros**: Simple, fast for small datasets, no artificial limits
- ❌ **Cons**: Memory usage grows unbounded, can exhaust system memory with large datasets
- **Best For**: Small to medium datasets (< 100K transactions), development, testing

#### Bounded Engine  
**Design**: Memory-capped using `lru::LruCache` for accounts, disputables, and processed tx IDs
- ✅ **Pros**: Predictable memory usage, handles large datasets, configurable limits
- ❌ **Cons**: LRU eviction may lose account data, potential data loss on eviction
- **Best For**: Large datasets with memory constraints, production with known memory budgets

#### Concurrent Engine
**Design**: Multi-threaded wrapper around bounded engine with `Arc<Mutex<BoundedEngine>>`

The concurrent engine uses **client-based assignment** to ensure all transactions for the same client are processed by the same worker thread, eliminating race conditions while maintaining parallelism.

⚠️ **CRITICAL SCALABILITY ISSUES** ⚠️

The concurrent engine has **fundamental architectural problems** that make it unsuitable for high-concurrency scenarios:

**Major Issues:**
1. **Global Lock Contention**: Every transaction from every stream must acquire the same `Arc<Mutex<>>` lock, creating a massive bottleneck
2. **Thread-per-Stream Model**: Spawns one OS thread per TCP stream, which doesn't scale beyond ~hundreds of streams
3. **Memory Explosion**: With 1000+ streams, thread stacks alone consume 2+ GB of RAM
4. **Serialized Processing**: Despite being "concurrent", the global lock serializes all transaction processing

**Performance Reality:**
- ✅ Works for single CSV files with client-based worker assignment
- ✅ Handles moderate concurrency (2-10 streams) reasonably well
- ❌ Lock contention actually makes it slower than single-threaded engines at scale

**For True High-Concurrency Processing:**
- Async/await architecture (Tokio) instead of threads
- Sharded state (multiple engines) instead of global locks
- Streaming with backpressure instead of buffering
- Lock-free or fine-grained locking strategies

#### Engine Selection Guide

| Use Case | Recommended Engine | Reason |
|----------|-------------------|---------|
| Small datasets (< 10K transactions) | `standard` | Simple, fast, no memory limits |
| Large datasets (> 100K transactions) | `bounded` | Memory-efficient, predictable resource usage |
| Memory-constrained environments | `bounded` with `--memory-limit-mb` | Auto-configured memory limits |
| Single CSV file processing | `standard` or `bounded` | Both are efficient and safe |
| Light concurrency (2-10 streams) | `concurrent` | Acceptable with limited streams |
| **HIGH CONCURRENCY (1000+ streams)** | **❌ None - Architecture Redesign Needed** | Current engines don't scale to this level |
| Production systems | `bounded` | Memory-safe and predictable |

### Error Handling

The engine handles various error conditions:

- **AccountFrozen**: Account is locked due to chargeback
- **InsufficientFunds**: Not enough funds for withdrawal or dispute
- **TransactionNotFound**: Referenced transaction doesn't exist
- **TransactionAlreadyDisputed**: Transaction is already under dispute
- **TransactionNotDisputed**: Trying to resolve/chargeback non-disputed transaction
- **ClientIdMismatch**: Client ID doesn't match original transaction
- **InvalidTransaction**: General validation errors (missing amount, negative values, etc.)

### Safety Features

- **Account Locking**: Accounts are permanently locked after chargebacks
- **Balance Validation**: Prevents overdrafts and negative balances
- **Transaction Uniqueness**: Ensures transaction IDs are unique
- **Client Validation**: Verifies client ownership of transactions
- **Dispute State Tracking**: Prevents duplicate disputes and invalid state transitions

### Benchmarking

Build the benchmarks binary and run synthetic load tests:

```bash
cargo build --release

# Standard engine - good for small datasets
./target/release/benchmark --engine standard -n 100000 --dispute-rate-percent 5

# Bounded engine - good for large datasets
./target/release/benchmark --engine bounded -n 200000 \
  --max-accounts 10000 --max-transactions 50000 --max-tx-ids 1000000

# Concurrent engine - limited scalability due to global lock
./target/release/benchmark --engine concurrent -n 500000 --streams 8 \
  --max-accounts 20000 --max-transactions 100000 --max-tx-ids 2000000

```

## Performance Characteristics

### Standard Engine
- **Throughput**: Excellent for small datasets
- **Memory**: Unbounded growth - can OOM on large datasets
- **Concurrency**: Single-threaded only

### Bounded Engine  
- **Throughput**: Good, consistent performance
- **Memory**: Bounded by configuration, predictable
- **Concurrency**: Single-threaded only

### Concurrent Engine
- **Throughput**: 
  - ✅ Good with 2-10 streams (client-based assignment helps)
  - ❌ Poor with 100+ streams (lock contention dominates)
  - ❌ Fails with 1000+ streams (thread exhaustion)
- **Memory**: Bounded + thread overhead (problematic at scale)
- **Concurrency**: Limited by global lock architecture

## Examples

### Basic Transaction Flow

```csv
type,client,tx,amount
deposit,1,1,100.0
withdrawal,1,2,25.0
dispute,1,1,
resolve,1,1,
```

Result: Client 1 has 75.0 available, 0.0 held, 75.0 total, unlocked

### Chargeback Scenario

```csv
type,client,tx,amount
deposit,1,1,50.0
dispute,1,1,
chargeback,1,1,
```

Result: Client 1 has 0.0 available, 0.0 held, 0.0 total, **locked**

### Error Cases

```csv
type,client,tx,amount
deposit,1,1,100.0
withdrawal,1,2,150.0    # Error: Insufficient funds
dispute,2,1,            # Error: Client ID mismatch
dispute,1,1,
dispute,1,1,            # Error: Already disputed
```

## Development

### Running Tests

```bash
cargo test
```

### Code Quality

```bash
# Check for compilation errors
cargo check

# Run clippy for linting
cargo clippy

# Format code
cargo fmt
```

### Dependencies

- `clap`: Command-line argument parsing
- `csv`: CSV file processing
- `rust_decimal`: Precise decimal arithmetic
- `serde`: Serialization/deserialization
- `thiserror`: Error handling
- `log` + `env_logger`: Logging
- `derive_more`: Derive macros
- `lru`: Memory-bounded caches for the bounded/concurrent engines

## Performance

- **Memory Efficient**: Uses `HashMap`/`HashSet` in standard mode; `lru::LruCache` to cap memory in bounded/concurrent modes
- **Zero-Copy**: Minimal data copying during processing
- **Streaming**: Processes CSV files line by line without loading entire file into memory
- **Error Recovery**: Continues processing after individual transaction failures

## Security Considerations

- **Input Validation**: All inputs are validated before processing
- **State Integrity**: Account balances are always consistent
- **Audit Trail**: All transactions are stored for future reference
- **Immutable History**: Transaction records cannot be modified after creation


## Future Improvements

The current CLI architecture has several limitations that require addressing for production use:

### State Persistence (Critical Missing Feature)
**Problem**: CLI processes only single CSV files without maintaining account history between runs.

```bash
# Day 1: Process initial transactions
./payments-engine day1_transactions.csv > day1_accounts.csv

# Day 2: All previous state is lost!
./payments-engine day2_transactions.csv > day2_accounts.csv
# ❌ Can't dispute Day 1 transactions, accounts start from zero
```

**Required Solution**:
```rust
pub trait PersistentEngine {
    /// Load previous state from disk before processing new transactions
    fn load_state(&mut self, state_dir: &Path) -> Result<(), Box<dyn std::error::Error>>;
    
    /// Save current state to disk after processing
    fn save_state(&self, state_dir: &Path) -> Result<(), Box<dyn std::error::Error>>;
}

// Enhanced CLI with persistence
./payments-engine day2_transactions.csv --state-dir ./payment_state
```

**What needs to be persisted**:
- Account balances and lock status
- Disputable transaction history (for future disputes)
- Processed transaction IDs (for duplicate detection)
- Metadata (last processed time, transaction counts)

**Current Impact**: 
- ❌ Can't process incremental daily transaction files
- ❌ Lose all dispute history between runs  
- ❌ Can't validate transaction ID uniqueness across days
- ❌ Not suitable for production batch processing

### High-Concurrency Architecture
To handle thousands of concurrent TCP streams, consider these architectural changes:

#### Async Architecture
```rust
// Replace thread-per-stream with async/await
pub async fn handle_concurrent_streams(&self, listener: TcpListener) {
    // Use Tokio for lightweight concurrency
}
```

#### Sharded State
```rust
// Replace global lock with sharded engines
pub struct ShardedEngine {
    shards: Vec<Arc<Mutex<BoundedEngine>>>,
}
```

#### Streaming with Backpressure
```rust
// Process transactions as they arrive, not in batches
pub async fn stream_transactions(&self, stream: TcpStream) {
    // Handle backpressure when processing can't keep up
}
```

### Production-Ready Enhancements
- **Database Integration**: Replace file-based persistence with proper database storage
- **Transaction Logging**: Immutable audit trail for all processed transactions
- **Monitoring & Metrics**: Real-time processing statistics and health checks
- **Configuration Management**: Environment-based configuration for different deployment contexts
- **Graceful Shutdown**: Proper cleanup and state saving on termination

These improvements would enable true production deployment, incremental processing, and high-concurrency scenarios without the current architectural bottlenecks.