use clap::Parser;
use rayon::prelude::*;
use simulator::{config, engine, output, scenarios};
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

    let config = match config::SimConfig::load(cli.config.as_deref()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };

    let scenarios = match scenarios::generate(&config) {
        Ok(scenarios) => scenarios,
        Err(err) => {
            eprintln!("Failed to generate scenarios: {err}");
            std::process::exit(1);
        }
    };
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
