use anyhow::Result;
use clap::{Parser, Subcommand};
use fork::historical_cpmm::artifacts::write_csv;
use fork::historical_cpmm::attack::evaluate_attacks;
use fork::historical_cpmm::config::{select_pools, DEFAULT_RPC_URL};
use fork::historical_cpmm::pipeline::{
    build_decoded_stage, collect_signatures_stage_with_floor, default_paths, ensure_paths,
    fetch_transactions_stage, rpc_client, run_all_with_floor,
};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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

    /// Optional floor: stop paginating once a signature is older than this
    /// many hours ago. Combine with a generous `--limit-per-pool` (e.g.
    /// 20000) to walk back exactly the requested window.
    #[arg(long, global = true)]
    since_hours_ago: Option<u64>,
}

#[derive(Subcommand, Debug)]
enum Command {
    CollectSignatures,
    FetchTransactions,
    DecodeSwaps,
    BuildDecoded,
    EvaluateAttacks {
        /// Minimum victim_amount_in below which a row is marked
        /// `below_candidate_threshold` and skipped.
        #[arg(long, default_value_t = 10_000)]
        min_victim: u128,
    },
    RunAll {
        /// Same threshold as `EvaluateAttacks::min_victim`.
        #[arg(long, default_value_t = 10_000)]
        min_victim: u128,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let pools = select_pools(&cli.pools)?;
    let paths = default_paths(cli.cache_root, cli.results_dir, cli.snapshot_root);
    ensure_paths(&paths)?;
    let rpc = rpc_client(&cli.rpc);

    let min_block_time = cli.since_hours_ago.map(|hours| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        now - (hours as i64) * 3600
    });

    match cli.command {
        Command::CollectSignatures => {
            let rows = collect_signatures_stage_with_floor(
                &rpc,
                &pools,
                cli.limit_per_pool,
                min_block_time,
                &paths,
            )?;
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
        Command::EvaluateAttacks { min_victim } => {
            let candidates_csv = paths.results_dir.join("historical_cpmm_candidates.csv");
            let stats = evaluate_attacks(
                &paths.decoded_csv(),
                &candidates_csv,
                cli.tx_cost_per_leg,
                Some(min_victim),
            )?;
            println!(
                "EvaluateAttacks: in={} out={} below={} replay_mismatch={} \
                 profitable={} unprofitable={} no_result={} invalid_state={}",
                stats.input_rows,
                stats.output_rows,
                stats.below_threshold,
                stats.replay_mismatch,
                stats.profitable,
                stats.unprofitable,
                stats.no_result,
                stats.invalid_state,
            );
        }
        Command::RunAll { min_victim } => {
            let (decoded, statuses, _summary) = run_all_with_floor(
                &rpc,
                &pools,
                cli.limit_per_pool,
                min_block_time,
                &paths,
                cli.tx_cost_per_leg,
            )?;
            println!(
                "Historical CPMM pipeline complete: {} decoded row(s), {} status row(s)",
                decoded.len(),
                statuses.len()
            );
            let candidates_csv = paths.results_dir.join("historical_cpmm_candidates.csv");
            let stats = evaluate_attacks(
                &paths.decoded_csv(),
                &candidates_csv,
                cli.tx_cost_per_leg,
                Some(min_victim),
            )?;
            println!(
                "EvaluateAttacks: in={} out={} below={} replay_mismatch={} \
                 profitable={} unprofitable={} no_result={} invalid_state={}",
                stats.input_rows,
                stats.output_rows,
                stats.below_threshold,
                stats.replay_mismatch,
                stats.profitable,
                stats.unprofitable,
                stats.no_result,
                stats.invalid_state,
            );
        }
    }

    Ok(())
}
