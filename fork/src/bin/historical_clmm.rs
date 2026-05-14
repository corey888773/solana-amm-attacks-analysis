use anyhow::Result;
use clap::{Parser, Subcommand};
use fork::historical_clmm::artifacts::write_csv;
use fork::historical_clmm::attack::{evaluate_live_attacks, LiveAttackConfig, DEFAULT_MIN_VICTIM};
use fork::historical_clmm::config::{select_pools, DEFAULT_RPC_URL};
use fork::historical_clmm::live::{
    build_live_candidate_readiness, live_candidates_csv, run_live_collect, LiveCollectConfig,
};
use fork::historical_clmm::pipeline::{
    build_decoded_stage, collect_signatures_stage, fetch_transactions_stage, probe_state_stage,
    rpc_client, run_all, PipelinePaths,
};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Collect and decode historical Raydium CLMM swap observations")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, default_value = DEFAULT_RPC_URL, global = true)]
    rpc: String,

    #[arg(long, default_value = "fork/cache/historical_clmm", global = true)]
    cache_root: PathBuf,

    #[arg(long, default_value = "results", global = true)]
    results_dir: PathBuf,

    /// Pool labels from the built-in CLMM registry. Empty means all configured pools.
    #[arg(long = "pool", global = true)]
    pools: Vec<String>,

    #[arg(long, default_value_t = 50, global = true)]
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
    ProbeState,
    RunAll,
    LiveCollect {
        #[arg(long, default_value_t = 28_800)]
        duration_seconds: u64,

        #[arg(long, default_value_t = 10)]
        interval_seconds: u64,

        #[arg(long, default_value_t = 50)]
        poll_limit: usize,
    },
    BuildLiveCandidates,
    EvaluateLiveAttacks {
        #[arg(long, default_value_t = 200)]
        max_steps: u64,

        #[arg(long, default_value_t = 100)]
        replay_tolerance_bps: u64,

        /// Minimum victim `amount_in` below which a row is marked
        /// `below_candidate_threshold` and skipped. Mirrors the CPMM
        /// evaluator floor — see `historical_clmm::attack::DEFAULT_MIN_VICTIM`
        /// for rationale.
        #[arg(long, default_value_t = DEFAULT_MIN_VICTIM)]
        min_victim: u128,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let rpc = rpc_client(&cli.rpc);
    let pools = select_pools(&cli.pools)?;
    let paths = PipelinePaths {
        cache_root: cli.cache_root,
        results_dir: cli.results_dir,
    };

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
                "Wrote {} decoded CLMM row(s) and {} status row(s)",
                decoded.len(),
                statuses.len()
            );
        }
        Command::ProbeState => {
            let rows = probe_state_stage(&rpc, &paths)?;
            println!(
                "Wrote {} CLMM state probe row(s) to {}",
                rows.len(),
                paths.state_probe_csv().display()
            );
        }
        Command::RunAll => {
            let (decoded, statuses, summary) = run_all(
                &rpc,
                &pools,
                cli.limit_per_pool,
                &paths,
                cli.tx_cost_per_leg,
            )?;
            println!(
                "Wrote {} decoded CLMM row(s), {} status row(s), {} summary row(s)",
                decoded.len(),
                statuses.len(),
                summary.len()
            );
        }
        Command::LiveCollect {
            duration_seconds,
            interval_seconds,
            poll_limit,
        } => {
            run_live_collect(
                &rpc,
                &pools,
                LiveCollectConfig {
                    cache_root: paths.cache_root,
                    results_dir: paths.results_dir,
                    duration_seconds,
                    interval_seconds,
                    poll_limit,
                    tx_cost_per_leg: cli.tx_cost_per_leg,
                },
            )?;
        }
        Command::BuildLiveCandidates => {
            let rows = build_live_candidate_readiness(&paths.results_dir)?;
            let output = live_candidates_csv(&paths.results_dir);
            write_csv(&output, &rows)?;
            let ready = rows.iter().filter(|row| row.live_candidate_ready).count();
            println!(
                "Wrote {} live CLMM readiness row(s) to {} ({} ready)",
                rows.len(),
                output.display(),
                ready
            );
        }
        Command::EvaluateLiveAttacks {
            max_steps,
            replay_tolerance_bps,
            min_victim,
        } => {
            let output_path = paths.results_dir.join("historical_clmm_candidates.csv");
            let rows = evaluate_live_attacks(&LiveAttackConfig {
                results_dir: paths.results_dir,
                max_steps,
                replay_tolerance_bps,
                tx_cost_per_leg: cli.tx_cost_per_leg,
                min_victim,
            })?;
            let evaluated = rows
                .iter()
                .filter(|row| row.model_status == "evaluated")
                .count();
            let profitable = rows.iter().filter(|row| row.attack_profitable).count();
            println!(
                "Wrote {} CLMM attack row(s) to {} ({} evaluated, {} profitable)",
                rows.len(),
                output_path.display(),
                evaluated,
                profitable
            );
        }
    }

    Ok(())
}
