use anyhow::Result;
use clap::{Parser, Subcommand};
use fork::historical_cpmm::artifacts::write_csv;
use fork::historical_cpmm::config::{select_pools, DEFAULT_RPC_URL};
use fork::historical_cpmm::pipeline::{
    build_decoded_stage, collect_signatures_stage, default_paths, ensure_paths,
    fetch_transactions_stage, rpc_client, run_all,
};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Collect and decode historical Raydium CPMM swap candidates")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, default_value = DEFAULT_RPC_URL, global = true)]
    rpc: String,

    #[arg(long, default_value = "fork/cache/historical_cpmm", global = true)]
    cache_root: PathBuf,

    #[arg(long, default_value = "fork/cache/pools", global = true)]
    snapshot_root: PathBuf,

    #[arg(long, default_value = "results", global = true)]
    results_dir: PathBuf,

    /// Pool labels from the built-in registry. Empty means all configured pools.
    #[arg(long = "pool", global = true)]
    pools: Vec<String>,

    #[arg(long, default_value_t = 100, global = true)]
    limit_per_pool: usize,

    #[arg(long, default_value_t = 0, global = true)]
    tx_cost_per_leg: u128,
}

#[derive(Subcommand, Debug)]
enum Command {
    CollectSignatures,
    FetchTransactions,
    DecodeSwaps,
    BuildDecoded,
    RunAll,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let pools = select_pools(&cli.pools)?;
    let paths = default_paths(cli.cache_root, cli.results_dir, cli.snapshot_root);
    ensure_paths(&paths)?;
    let rpc = rpc_client(&cli.rpc);

    match cli.command {
        Command::CollectSignatures => {
            let rows = collect_signatures_stage(&rpc, &pools, cli.limit_per_pool, &paths)?;
            println!(
                "Wrote {} signature row(s) to {}",
                rows.len(),
                paths.signatures_csv().display()
            );
        }
        Command::FetchTransactions => {
            let rows = fetch_transactions_stage(&rpc, &paths)?;
            println!(
                "Fetched/cached transaction JSON for {} successful signature row(s)",
                rows.iter().filter(|row| row.err.is_none()).count()
            );
        }
        Command::DecodeSwaps | Command::BuildDecoded => {
            let (decoded, statuses, summary) =
                build_decoded_stage(&rpc, &paths, cli.tx_cost_per_leg)?;
            write_csv(&paths.summary_csv(), &summary)?;
            println!(
                "Wrote {} decoded row(s) and {} status row(s)",
                decoded.len(),
                statuses.len()
            );
        }
        Command::RunAll => {
            let (decoded, statuses, _summary) = run_all(
                &rpc,
                &pools,
                cli.limit_per_pool,
                &paths,
                cli.tx_cost_per_leg,
            )?;
            println!(
                "Historical CPMM pipeline complete: {} decoded row(s), {} status row(s)",
                decoded.len(),
                statuses.len()
            );
        }
    }

    Ok(())
}
