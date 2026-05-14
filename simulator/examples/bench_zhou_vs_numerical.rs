use amm_math::multi_fee::MultiFeeConfig;
use amm_math::sandwich::closed_form::compute_closed_form_sandwich;
use amm_math::sandwich::numerical::{compute_grid_sandwich, compute_numerical_sandwich};
use amm_math::types::SandwichResult;
use clap::Parser;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "Generate closed-form vs numerical sandwich benchmark CSV")]
struct Args {
    /// Output CSV path.
    #[arg(short, long, default_value = "results/zhou_vs_numerical.csv")]
    output: PathBuf,

    /// Maximum grid samples for the oracle column.
    #[arg(long, default_value_t = 10_000)]
    grid_steps: u64,
}

#[derive(Debug, Serialize)]
struct BenchRow {
    reserve_in: u128,
    reserve_out: u128,
    liquidity_depth_label: &'static str,
    fee_bps: u16,
    victim_bps_of_reserve: u64,
    victim_amount: u128,
    tx_cost_per_leg: u128,
    tx_cost_total: u128,
    closed_form_frontrun: u128,
    numerical_frontrun: u128,
    grid_frontrun: u128,
    closed_form_gross_profit: i128,
    numerical_gross_profit: i128,
    grid_gross_profit: i128,
    closed_form_net_profit: i128,
    numerical_net_profit: i128,
    grid_net_profit: i128,
    closed_form_profitable: bool,
    numerical_profitable: bool,
    grid_profitable: bool,
    profit_gap_abs: i128,
    profit_gap_rel: f64,
    frontrun_gap_abs: i128,
    frontrun_gap_rel: f64,
    numerical_grid_profit_gap: i128,
}

fn zero_result() -> SandwichResult {
    SandwichResult {
        frontrun_amount: 0,
        frontrun_output: 0,
        backrun_output: 0,
        victim_extra_slippage_bps: 0,
        gross_profit: 0,
        net_profit: 0,
        is_profitable: false,
    }
}

fn abs_diff_u128(a: u128, b: u128) -> i128 {
    if a >= b {
        (a - b).min(i128::MAX as u128) as i128
    } else {
        -((b - a).min(i128::MAX as u128) as i128)
    }
}

fn rel_gap(numerator: i128, denominator: i128) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator.unsigned_abs() as f64
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }

    // Depth labels are relative toy scales for hypothesis plots, not live TVL.
    let pools = [
        ("thin", 100_000u128, 100_000u128),
        ("medium", 1_000_000u128, 1_000_000u128),
        ("deep", 10_000_000u128, 10_000_000u128),
        ("very_deep", 100_000_000u128, 100_000_000u128),
    ];
    let fee_bps_values = [0u16, 5, 10, 30, 50, 100, 250];
    let victim_bps_values = [10u64, 50, 100, 250, 500, 1_000];
    let tx_cost_per_leg_values = [0u128, 10, 100, 1_000];

    let mut writer = csv::Writer::from_path(&args.output)?;

    for (label, reserve_in, reserve_out) in pools {
        for fee_bps in fee_bps_values {
            let cfg = MultiFeeConfig::single(u64::from(fee_bps), 10_000);
            for victim_bps in victim_bps_values {
                let victim_amount = reserve_in * u128::from(victim_bps) / 10_000;
                if victim_amount == 0 {
                    continue;
                }

                for tx_cost_per_leg in tx_cost_per_leg_values {
                    let tx_cost_total = tx_cost_per_leg * 2;
                    let closed = compute_closed_form_sandwich(
                        victim_amount,
                        reserve_in,
                        reserve_out,
                        fee_bps,
                        tx_cost_total,
                    )
                    .unwrap_or_else(zero_result);
                    let numerical = compute_numerical_sandwich(
                        reserve_in,
                        reserve_out,
                        victim_amount,
                        &cfg,
                        tx_cost_per_leg,
                    )
                    .unwrap_or_else(zero_result);
                    let grid = compute_grid_sandwich(
                        reserve_in,
                        reserve_out,
                        victim_amount,
                        &cfg,
                        tx_cost_per_leg,
                        args.grid_steps,
                    )
                    .unwrap_or_else(zero_result);

                    let profit_gap_abs = numerical.net_profit - closed.net_profit;
                    let frontrun_gap_abs =
                        abs_diff_u128(numerical.frontrun_amount, closed.frontrun_amount);

                    writer.serialize(BenchRow {
                        reserve_in,
                        reserve_out,
                        liquidity_depth_label: label,
                        fee_bps,
                        victim_bps_of_reserve: victim_bps,
                        victim_amount,
                        tx_cost_per_leg,
                        tx_cost_total,
                        closed_form_frontrun: closed.frontrun_amount,
                        numerical_frontrun: numerical.frontrun_amount,
                        grid_frontrun: grid.frontrun_amount,
                        closed_form_gross_profit: closed.gross_profit,
                        numerical_gross_profit: numerical.gross_profit,
                        grid_gross_profit: grid.gross_profit,
                        closed_form_net_profit: closed.net_profit,
                        numerical_net_profit: numerical.net_profit,
                        grid_net_profit: grid.net_profit,
                        closed_form_profitable: closed.is_profitable,
                        numerical_profitable: numerical.is_profitable,
                        grid_profitable: grid.is_profitable,
                        profit_gap_abs,
                        profit_gap_rel: rel_gap(profit_gap_abs, numerical.net_profit),
                        frontrun_gap_abs,
                        frontrun_gap_rel: rel_gap(
                            frontrun_gap_abs,
                            numerical.frontrun_amount as i128,
                        ),
                        numerical_grid_profit_gap: numerical.net_profit - grid.net_profit,
                    })?;
                }
            }
        }
    }

    writer.flush()?;
    eprintln!("wrote {}", args.output.display());
    Ok(())
}
