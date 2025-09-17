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

- **Standard**: In-memory `HashMap`/`HashSet`. Best for small/medium datasets. Unlimited by default.
- **Bounded**: Memory-capped using `lru::LruCache` for accounts, disputables, and processed tx IDs. Best for large datasets on a single machine.
- **Concurrent**: Threaded variant built on the bounded engine with a shared `Arc<Mutex<...>>`; suitable for high-throughput ingestion from multiple readers/streams.

You can select an engine via `--engine` or let `--memory-limit-mb` auto-size a bounded configuration. When using bounded/concurrent modes, entries may be evicted per LRU; only currently cached accounts are emitted in the final CSV.

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
./target/release/benchmark --engine standard -n 100000 --dispute-rate-percent 5
./target/release/benchmark --engine bounded -n 200000 \
  --max-accounts 10000 --max-transactions 50000 --max-tx-ids 1000000
./target/release/benchmark --engine concurrent -n 500000 --streams 8 \
  --max-accounts 20000 --max-transactions 100000 --max-tx-ids 2000000
```

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