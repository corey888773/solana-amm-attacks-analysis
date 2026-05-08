use amm_math::multi_fee::{compute_swap_multi_fee, CreatorFeeMode, MultiFeeConfig};
use amm_math::sandwich::compute_numerical_sandwich;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CANDIDATE_THRESHOLD_BPS: u64 = 5;

#[derive(Parser)]
#[command(about = "Evaluate decoded historical Raydium CPMM swap candidates")]
struct Cli {
    /// Input CSV with decoded swap instruction and pool pre-state columns.
    #[arg(short, long)]
    input: PathBuf,

    /// Output CSV with counterfactual sandwich profitability columns.
    #[arg(short, long, default_value = "results/historical_cpmm_candidates.csv")]
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CandidateRow {
    pool_type: String,
    pool_label: String,
    pool_address: String,
    slot: u64,
    signature: String,
    #[serde(default)]
    block_time: Option<i64>,
    instruction_index: u32,
    direction: String,
    amount_in: u128,
    #[serde(default)]
    min_amount_out: Option<u128>,
    #[serde(default)]
    actual_amount_out: Option<u128>,
    reserve_in_before: u128,
    reserve_out_before: u128,
    trade_fee_rate: u64,
    #[serde(default)]
    creator_fee_rate: u64,
    fee_denominator: u64,
    #[serde(default)]
    creator_fee_mode: Option<String>,
    #[serde(default)]
    tx_cost_per_leg: u128,
}

#[derive(Debug, Serialize)]
struct EvaluatedRow {
    pool_type: String,
    pool_label: String,
    pool_address: String,
    slot: u64,
    signature: String,
    block_time: Option<i64>,
    instruction_index: u32,
    direction: String,
    amount_in: u128,
    min_amount_out: Option<u128>,
    actual_amount_out: Option<u128>,
    reserve_in_before: u128,
    reserve_out_before: u128,
    trade_fee_rate: u64,
    creator_fee_rate: u64,
    fee_denominator: u64,
    creator_fee_mode: String,
    tx_cost_per_leg: u128,
    tx_cost_total: u128,
    fair_amount_out: Option<u128>,
    victim_size_bps_of_reserve: u64,
    victim_slippage_tolerance_bps: Option<u64>,
    optimal_frontrun: u128,
    attacker_gross_profit: i128,
    attacker_net_profit: i128,
    victim_loss_absolute: u128,
    victim_extra_slippage_bps: u64,
    attack_feasible: bool,
    attack_profitable: bool,
    below_candidate_threshold: bool,
    inclusion_reason: String,
    exclusion_reason: String,
}

fn bps_u128(numerator: u128, denominator: u128) -> u64 {
    if denominator == 0 {
        return 0;
    }
    (numerator.saturating_mul(10_000) / denominator).min(u64::MAX as u128) as u64
}

fn parse_creator_fee_mode(mode: Option<&str>, creator_fee_rate: u64) -> CreatorFeeMode {
    match mode.unwrap_or("on_input") {
        "disabled" => CreatorFeeMode::Disabled,
        "on_output" => CreatorFeeMode::OnOutput,
        "on_input" => {
            if creator_fee_rate == 0 {
                CreatorFeeMode::Disabled
            } else {
                CreatorFeeMode::OnInput
            }
        }
        _ => CreatorFeeMode::OnInput,
    }
}

fn creator_fee_mode_label(mode: CreatorFeeMode) -> &'static str {
    match mode {
        CreatorFeeMode::Disabled => "disabled",
        CreatorFeeMode::OnInput => "on_input",
        CreatorFeeMode::OnOutput => "on_output",
    }
}

fn evaluate(row: CandidateRow) -> EvaluatedRow {
    let creator_fee_mode =
        parse_creator_fee_mode(row.creator_fee_mode.as_deref(), row.creator_fee_rate);
    let cfg = MultiFeeConfig {
        trade_fee_rate: row.trade_fee_rate,
        creator_fee_rate: row.creator_fee_rate,
        fee_denominator: row.fee_denominator,
        creator_fee_mode,
    };
    let tx_cost_total = row.tx_cost_per_leg.saturating_mul(2);
    let victim_size_bps_of_reserve = bps_u128(row.amount_in, row.reserve_in_before);
    let below_candidate_threshold = victim_size_bps_of_reserve < CANDIDATE_THRESHOLD_BPS;

    let fair = compute_swap_multi_fee(
        row.reserve_in_before,
        row.reserve_out_before,
        row.amount_in,
        &cfg,
    );
    let fair_amount_out = fair.as_ref().map(|swap| swap.amount_out);
    let victim_slippage_tolerance_bps = match (fair_amount_out, row.min_amount_out) {
        (Some(fair), Some(min_out)) if fair > min_out => Some(bps_u128(fair - min_out, fair)),
        (Some(_), Some(_)) => Some(0),
        _ => None,
    };

    let result = compute_numerical_sandwich(
        row.reserve_in_before,
        row.reserve_out_before,
        row.amount_in,
        &cfg,
        row.tx_cost_per_leg,
    );

    let mut exclusion_reason = String::new();
    if fair_amount_out.is_none() {
        exclusion_reason = "invalid_pre_state_or_swap".to_string();
    } else if row.min_amount_out.is_none() {
        exclusion_reason = "missing_slippage_threshold".to_string();
    }

    let (
        optimal_frontrun,
        attacker_gross_profit,
        attacker_net_profit,
        victim_loss_absolute,
        victim_extra_slippage_bps,
        attack_feasible,
        attack_profitable,
    ) = match result {
        Some(result) => {
            let feasible = victim_slippage_tolerance_bps
                .map(|limit| result.victim_extra_slippage_bps <= limit)
                .unwrap_or(false);
            (
                result.frontrun_amount,
                result.gross_profit,
                result.net_profit,
                fair_amount_out
                    .map(|fair| {
                        fair.saturating_mul(u128::from(result.victim_extra_slippage_bps)) / 10_000
                    })
                    .unwrap_or(0),
                result.victim_extra_slippage_bps,
                feasible,
                result.is_profitable,
            )
        }
        None => {
            if exclusion_reason.is_empty() {
                exclusion_reason = "no_profitable_attack".to_string();
            }
            (0, 0, 0, 0, 0, false, false)
        }
    };

    let inclusion_reason = if exclusion_reason.is_empty() {
        "decoded_pre_state_available".to_string()
    } else {
        String::new()
    };

    EvaluatedRow {
        pool_type: row.pool_type,
        pool_label: row.pool_label,
        pool_address: row.pool_address,
        slot: row.slot,
        signature: row.signature,
        block_time: row.block_time,
        instruction_index: row.instruction_index,
        direction: row.direction,
        amount_in: row.amount_in,
        min_amount_out: row.min_amount_out,
        actual_amount_out: row.actual_amount_out,
        reserve_in_before: row.reserve_in_before,
        reserve_out_before: row.reserve_out_before,
        trade_fee_rate: row.trade_fee_rate,
        creator_fee_rate: row.creator_fee_rate,
        fee_denominator: row.fee_denominator,
        creator_fee_mode: creator_fee_mode_label(creator_fee_mode).to_string(),
        tx_cost_per_leg: row.tx_cost_per_leg,
        tx_cost_total,
        fair_amount_out,
        victim_size_bps_of_reserve,
        victim_slippage_tolerance_bps,
        optimal_frontrun,
        attacker_gross_profit,
        attacker_net_profit,
        victim_loss_absolute,
        victim_extra_slippage_bps,
        attack_feasible,
        attack_profitable,
        below_candidate_threshold,
        inclusion_reason,
        exclusion_reason,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut reader = csv::Reader::from_path(&cli.input)?;
    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut writer = csv::Writer::from_path(&cli.output)?;

    let mut rows = 0usize;
    for row in reader.deserialize::<CandidateRow>() {
        writer.serialize(evaluate(row?))?;
        rows += 1;
    }
    writer.flush()?;

    eprintln!(
        "Evaluated {} historical CPMM candidate row(s) into {}",
        rows,
        cli.output.display()
    );
    Ok(())
}
