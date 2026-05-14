use clap::Parser;
use rayon::prelude::*;
use simulator::config::SimConfig;
use simulator::output::SimulationRecord;
use simulator::{engine, output, scenarios};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Generate synthetic-vs-Raydium real-pool comparison CSV")]
struct Cli {
    /// Config with [real_pool] and optional victim/slippage sweep.
    #[arg(short, long, default_value = "configs/sweep_real_pool.toml")]
    config: String,

    /// Output CSV path.
    #[arg(short, long, default_value = "results/real_pool_comparison.csv")]
    output: PathBuf,

    /// Use parallel execution for scenario rows.
    #[arg(long)]
    parallel: bool,
}

fn run_records(
    scenarios: &[scenarios::Scenario],
    config: &SimConfig,
    source: &str,
    use_parallel: bool,
) -> Vec<SimulationRecord> {
    let mut records: Vec<SimulationRecord> = if use_parallel {
        scenarios
            .par_iter()
            .flat_map(|scenario| engine::run_scenario(scenario, config))
            .collect()
    } else {
        scenarios
            .iter()
            .flat_map(|scenario| engine::run_scenario(scenario, config))
            .collect()
    };

    for record in &mut records {
        record.source = source.to_string();
    }

    records
}

fn main() {
    let cli = Cli::parse();

    let config = match SimConfig::load(Some(&cli.config)) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };

    if config.real_pool.is_none() {
        eprintln!("compare-real-pool requires a config with [real_pool]");
        std::process::exit(1);
    }

    let real_scenarios = match scenarios::generate(&config) {
        Ok(scenarios) => scenarios,
        Err(err) => {
            eprintln!("Failed to generate real-pool scenarios: {err}");
            std::process::exit(1);
        }
    };

    let synthetic_scenarios: Vec<scenarios::Scenario> = real_scenarios
        .iter()
        .map(|scenario| scenarios::Scenario {
            pool_reserve_a: scenario.pool_reserve_a,
            pool_reserve_b: scenario.pool_reserve_b,
            pool_fee_bps: scenario.pool_fee_bps,
            fee_config: scenario.fee_config,
            victim_swap_amount: scenario.victim_swap_amount,
            victim_slippage_bps: scenario.victim_slippage_bps,
            tx_cost_per_leg: scenario.tx_cost_per_leg,
            tx_cost_total: scenario.tx_cost_total,
            tx_cost_per_leg_lamports: scenario.tx_cost_per_leg_lamports,
            tx_cost_total_lamports: scenario.tx_cost_total_lamports,
            pool_label: format!("synthetic_matched_state__{}", scenario.pool_label),
        })
        .collect();

    let use_parallel = cli.parallel || config.sweep.as_ref().map_or(false, |s| s.parallel);

    let mut records = run_records(
        &synthetic_scenarios,
        &config,
        "synthetic_cpmm_matched_state",
        use_parallel,
    );
    records.extend(run_records(
        &real_scenarios,
        &config,
        "raydium_cpmm_snapshot",
        use_parallel,
    ));

    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    output::write_csv(&records, &cli.output).expect("Failed to write CSV");
    eprintln!(
        "Wrote {} comparison row(s) to {}",
        records.len(),
        cli.output.display()
    );
}
