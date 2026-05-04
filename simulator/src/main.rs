mod config;
mod engine;
mod output;
mod real_pool;
mod scenarios;

use clap::Parser;
use rayon::prelude::*;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mev-sim",
    about = "MEV sandwich attack simulator for AMM pools"
)]
struct Cli {
    /// Path to TOML config file
    #[arg(short, long)]
    config: Option<String>,

    /// Output CSV path
    #[arg(short, long, default_value = "results/output.csv")]
    output: PathBuf,

    /// Use parallel execution for sweep (overrides config)
    #[arg(long)]
    parallel: bool,
}

fn main() {
    let cli = Cli::parse();

    let config = config::SimConfig::load(cli.config.as_deref()).expect("Failed to load config");

    let scenarios = scenarios::generate(&config);
    eprintln!(
        "Generated {} scenario(s), seed={}, direction={:?}",
        scenarios.len(),
        config.simulation.seed,
        config.victim.direction,
    );

    let use_parallel = cli.parallel || config.sweep.as_ref().map_or(false, |s| s.parallel);

    let records: Vec<output::SimulationRecord> = if use_parallel {
        scenarios
            .par_iter()
            .flat_map(|s| engine::run_scenario(s, &config))
            .collect()
    } else {
        scenarios
            .iter()
            .flat_map(|s| engine::run_scenario(s, &config))
            .collect()
    };

    eprintln!("Completed {} simulation(s)", records.len());

    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    output::write_csv(&records, &cli.output).expect("Failed to write CSV");

    eprintln!("Results written to {}", cli.output.display());
}
