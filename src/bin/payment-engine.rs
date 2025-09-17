use clap::Parser;
use std::path::PathBuf;

use payment_engine::{EngineConfig, PaymentsEngine};

/// Payment engine cli tool.
/// Reads transactions from a CSV file, processes them, and outputs the final state of client accounts.
/// Usage: payments-engine <input_file> [--output <output_file>] [--log-level <level>]
/// <input_file>: Path to the input CSV file containing transactions.
/// --output <output_file>: Optional path to the output CSV file (defaults to stdout).
/// --log-level <level>: Optional log level (e.g., info, debug, warn
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(name = "payments-engine")]
struct Args {
    /// Path to the input CSV file
    #[arg(help = "transactions.csv file path")]
    input_file: PathBuf,

    /// Output file path (defaults to stdout)
    #[arg(short, long, help = "Output CSV file path (defaults to stdout)")]
    output: Option<PathBuf>,

    /// Log level (e.g., info, debug, warn)
    #[arg(short, long, help = "Log level (e.g., info, debug, warn)")]
    log_level: Option<String>,

    /// Engine type to use
    #[arg(
        short,
        long,
        help = "Engine type: standard, bounded, or concurrent (defaults to standard)"
    )]
    engine: Option<String>,

    /// Maximum number of accounts in memory (for bounded/concurrent engines)
    #[arg(
        long,
        help = "Maximum accounts in memory (bounded/concurrent only, default: 10000)"
    )]
    max_accounts: Option<usize>,

    /// Maximum number of disputable transactions in memory (for bounded/concurrent engines)
    #[arg(
        long,
        help = "Maximum disputable transactions in memory (bounded/concurrent only, default: 50000)"
    )]
    max_transactions: Option<usize>,

    /// Maximum number of processed transaction IDs in memory (for bounded/concurrent engines)
    #[arg(
        long,
        help = "Maximum processed transaction IDs in memory (bounded/concurrent only, default: 1000000)"
    )]
    max_tx_ids: Option<usize>,

    /// Automatically configure engine based on available memory in MB
    #[arg(
        long,
        help = "Auto-configure bounded engine for given memory limit in MB (overrides other max-* options)"
    )]
    memory_limit_mb: Option<usize>,
}

fn init_logger(log_level: &str) {
    let level = match log_level.to_lowercase().as_str() {
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => {
            eprintln!(
                "Invalid log level '{}'. Using 'info' as default.",
                log_level
            );
            log::LevelFilter::Info
        }
    };

    env_logger::Builder::from_default_env()
        .filter_level(level)
        .format_timestamp_secs()
        .init();
}

fn main() {
    let args = Args::parse();
    let log_level = args.log_level.unwrap_or_else(|| "info".to_string());
    init_logger(&log_level);

    let input_path = args.input_file;
    if !input_path.exists() {
        log::error!("Input file does not exist: {:?}", input_path);
        std::process::exit(1);
    }
    if input_path.extension().is_none_or(|ext| ext != "csv") {
        log::error!("Input file is not a CSV file: {:?}", input_path);
        std::process::exit(1);
    }

    let config = EngineConfig::from_cli_params(
        args.engine.as_deref(),
        args.max_accounts,
        args.max_transactions,
        args.max_tx_ids,
        args.memory_limit_mb,
    );
    let mut engine = PaymentsEngine::new(config);

    let engine_info = engine.get_engine_info();
    log::info!(
        "Using {} engine (memory bounded: {}, concurrent: {})",
        engine_info.engine_type,
        engine_info.memory_bounded,
        engine_info.concurrent
    );
    if let Some(limits) = &engine_info.memory_limits {
        log::info!(
            "Memory limits - accounts: {}, transactions: {}, tx_ids: {}",
            limits.max_accounts,
            limits.max_disputable_transactions,
            limits.max_processed_tx_ids
        );
    }

    engine
        .process_transactions_from_file(&input_path)
        .unwrap_or_else(|e| {
            log::error!("Failed to process transactions: {}", e);
            std::process::exit(1);
        });

    let final_info = engine.get_engine_info();
    log::info!(
        "Processing completed. Final account count: {}",
        final_info.account_count
    );
    if let Some(tx_count) = final_info.transaction_count {
        log::info!("Disputable transactions in memory: {}", tx_count);
    }
    let output_path = args.output;
    if let Some(path) = output_path {
        let file = std::fs::File::create(&path).unwrap_or_else(|e| {
            log::error!("Failed to create output file {:?}: {}", path, e);
            std::process::exit(1);
        });
        let writer = std::io::BufWriter::new(file);
        engine.write_accounts_csv(writer).unwrap_or_else(|e| {
            log::error!("Failed to write accounts to CSV: {}", e);
            std::process::exit(1);
        });
        log::info!("Accounts written to {:?}", path);
    } else {
        let writer = std::io::stdout();
        engine.write_accounts_csv(writer).unwrap_or_else(|e| {
            log::error!("Failed to write accounts to stdout: {}", e);
            std::process::exit(1);
        });
    }
}
