use clap::Parser;
use payment_engine::PaymentEngineBenchmark;

#[derive(Parser, Debug)]
#[command(author, version, about = "Run payment engine benchmarks", long_about = None)]
struct BenchArgs {
    /// Number of transactions to generate
    #[arg(short = 'n', long, default_value_t = 10000)]
    transactions: usize,

    /// Dispute rate in percent (e.g., 5 for 5%)
    #[arg(short = 'd', long, default_value_t = 5.0)]
    dispute_rate_percent: f32,

    /// Engine to benchmark: standard | bounded | concurrent
    #[arg(short, long, default_value = "standard")]
    engine: String,

    /// Max accounts (for bounded/concurrent)
    #[arg(long, default_value_t = 1000)]
    max_accounts: usize,

    /// Max disputable transactions (for bounded/concurrent)
    #[arg(long, default_value_t = 2000)]
    max_transactions: usize,

    /// Max processed tx ids (for bounded/concurrent)
    #[arg(long, default_value_t = 50000)]
    max_tx_ids: usize,

    /// Number of streams (for concurrent)
    #[arg(long, default_value_t = 4)]
    streams: usize,
}

fn main() {
    let args = BenchArgs::parse();
    let dispute_rate = args.dispute_rate_percent / 100.0;

    match args.engine.as_str() {
        "standard" => {
            let result = PaymentEngineBenchmark::benchmark_standard_engine(
                args.transactions,
                dispute_rate,
                args.max_accounts,
            );
            result.print_summary();
        }
        "bounded" => {
            let result = PaymentEngineBenchmark::benchmark_bounded_engine(
                args.transactions,
                dispute_rate,
                args.max_accounts,
                args.max_transactions,
                args.max_tx_ids,
                args.max_accounts,
            );
            result.print_summary();
        }
        "concurrent" => {
            let result = PaymentEngineBenchmark::benchmark_concurrent_engine(
                args.transactions,
                dispute_rate,
                args.streams,
                args.max_accounts,
                args.max_transactions,
                args.max_tx_ids,
                args.max_accounts,
            );
            result.print_summary();
        }
        other => {
            eprintln!(
                "Unknown engine: {} (use standard|bounded|concurrent)",
                other
            );
            std::process::exit(2);
        }
    }
}
