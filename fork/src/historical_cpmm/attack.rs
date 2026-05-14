//! CPMM sandwich evaluator. Reads `historical_cpmm_decoded.csv` and produces
//! `historical_cpmm_candidates.csv` with sandwich profitability per row.
//!
//! Mirrors the structure of the CLMM evaluator (`historical_clmm/attack.rs`)
//! but is substantially simpler: CPMM swap math is closed-form in
//! `amm-math::cpmm::multi_fee`, so no tick-array reconstruction is needed.

use crate::historical_cpmm::artifacts::{write_csv, DecodedCandidateRow};
use amm_math::cpmm::multi_fee::{compute_swap_multi_fee, CreatorFeeMode, MultiFeeConfig};
use amm_math::sandwich::numerical::compute_numerical_sandwich;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MODEL_VERSION: &str = "cpmm_multi_fee_v1";
const REPLAY_TOLERANCE_BPS: u64 = 100;
/// Below this victim `amount_in` we mark the row `below_candidate_threshold`
/// and skip the sandwich solve.
///
/// Rationale for `10_000` base units (mirrors CLMM evaluator):
/// 1. Economic floor — for 6-decimal stablecoins `10_000` ≈ `$0.01`; for
///    9-decimal WSOL ≈ `1e-5 SOL ≈ $0.0015` at `$150/SOL`. Below this,
///    swaps are sub-cent dust (wallet sweeps, rounding, test traffic),
///    not adversarial trade flow.
/// 2. Cost-coverage floor — sandwich net profit must cover
///    `2 * tx_cost_per_leg` (~`0.165` USDC at default
///    `configs/default.toml`); a `10_000`-unit victim is structurally
///    loss-making for any frontrun size.
/// 3. Numerical floor — ternary search degenerates when victim price
///    impact rounds to zero in `u128` arithmetic.
///
/// Overridable via `--min-victim` for sensitivity analysis.
const DEFAULT_MIN_VICTIM: u128 = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CpmmAttackRow {
    pub pool_type: String,
    pub pool_label: String,
    pub pool_address: String,
    pub slot: u64,
    pub signature: String,
    pub block_time: Option<i64>,
    pub instruction_index: u32,
    pub direction: String,

    pub amount_in: u128,
    pub victim_amount_in: u128,
    pub min_amount_out: Option<u128>,
    pub actual_amount_out: Option<u128>,
    pub reserve_in_before: u128,
    pub reserve_out_before: u128,

    pub trade_fee_rate: u64,
    pub creator_fee_rate: u64,
    pub fee_denominator: u64,
    pub creator_fee_mode: String,
    pub tx_cost_per_leg: u128,

    pub model_version: String,
    pub model_status: String,
    pub rejection_reason: Option<String>,
    pub fair_amount_out: Option<u128>,
    pub replay_error_bps: Option<u64>,

    pub best_attempt_frontrun: u128,
    pub best_attempt_frontrun_output: u128,
    pub best_attempt_backrun_output: u128,
    pub best_attempt_gross_profit: i128,
    pub best_attempt_net_profit: i128,
    pub best_attempt_victim_extra_slippage_bps: u64,
    pub best_attempt_feasible: bool,

    pub optimal_frontrun: u128,
    pub frontrun_output: u128,
    pub backrun_output: u128,
    pub attacker_gross_profit: i128,
    pub attacker_net_profit: i128,
    pub victim_extra_slippage_bps: u64,
    pub attack_feasible: bool,
    pub attack_profitable: bool,

    pub below_candidate_threshold: bool,
}

pub fn evaluate_attacks(
    decoded_csv: &Path,
    out_csv: &Path,
    tx_cost_per_leg: u128,
    min_victim_amount: Option<u128>,
) -> Result<EvaluateStats> {
    let min_victim = min_victim_amount.unwrap_or(DEFAULT_MIN_VICTIM);
    let mut rdr = csv::Reader::from_path(decoded_csv)
        .with_context(|| format!("open {}", decoded_csv.display()))?;
    let mut rows: Vec<CpmmAttackRow> = Vec::new();
    let mut stats = EvaluateStats::default();

    for record in rdr.deserialize::<DecodedCandidateRow>() {
        let dec = record.context("deserialize decoded row")?;
        stats.input_rows += 1;

        let below = dec.amount_in < min_victim;
        let mut out = base_row(&dec, tx_cost_per_leg, below);

        if below {
            out.model_status = "rejected".into();
            out.rejection_reason = Some("below_candidate_threshold".into());
            stats.below_threshold += 1;
            rows.push(out);
            continue;
        }

        let cfg = MultiFeeConfig {
            trade_fee_rate: dec.trade_fee_rate,
            creator_fee_rate: dec.creator_fee_rate,
            fee_denominator: dec.fee_denominator,
            creator_fee_mode: parse_mode(&dec.creator_fee_mode),
        };

        // 1) Replay (no-attack) — model's prediction of victim output
        let fair = compute_swap_multi_fee(
            dec.reserve_in_before,
            dec.reserve_out_before,
            dec.amount_in,
            &cfg,
        );
        if let Some(ref f) = fair {
            out.fair_amount_out = Some(f.amount_out);
            if let Some(actual) = dec.actual_amount_out {
                if actual > 0 {
                    let diff = f.amount_out.abs_diff(actual);
                    let bps = diff.saturating_mul(10_000) / actual;
                    out.replay_error_bps = Some(bps.min(u64::MAX as u128) as u64);
                    if bps > REPLAY_TOLERANCE_BPS as u128 {
                        out.model_status = "rejected".into();
                        out.rejection_reason = Some("victim_replay_mismatch".into());
                        stats.replay_mismatch += 1;
                        rows.push(out);
                        continue;
                    }
                }
            }
        } else {
            out.model_status = "rejected".into();
            out.rejection_reason = Some("invalid_pool_state".into());
            stats.invalid_state += 1;
            rows.push(out);
            continue;
        }

        // 2) Sandwich
        let sandwich = compute_numerical_sandwich(
            dec.reserve_in_before,
            dec.reserve_out_before,
            dec.amount_in,
            &cfg,
            tx_cost_per_leg,
        );

        match sandwich {
            None => {
                // `compute_numerical_sandwich` returns None when no positive-
                // profit frontrun exists. Semantically: best attempt is
                // unprofitable. Diagnostic best_attempt_* fields remain 0
                // because the optimizer doesn't expose the loss-making search
                // path; richer diagnostics would require a grid oracle pass.
                out.model_status = "evaluated".into();
                out.rejection_reason = Some("best_attempt_unprofitable".into());
                stats.unprofitable += 1;
            }
            Some(sr) => {
                out.best_attempt_frontrun = sr.frontrun_amount;
                out.best_attempt_frontrun_output = sr.frontrun_output;
                out.best_attempt_backrun_output = sr.backrun_output;
                out.best_attempt_gross_profit = sr.gross_profit;
                out.best_attempt_net_profit = sr.net_profit;
                out.best_attempt_victim_extra_slippage_bps = sr.victim_extra_slippage_bps;
                out.best_attempt_feasible = true;

                if sr.is_profitable {
                    out.attack_profitable = true;
                    out.attack_feasible = true;
                    out.optimal_frontrun = sr.frontrun_amount;
                    out.frontrun_output = sr.frontrun_output;
                    out.backrun_output = sr.backrun_output;
                    out.attacker_gross_profit = sr.gross_profit;
                    out.attacker_net_profit = sr.net_profit;
                    out.victim_extra_slippage_bps = sr.victim_extra_slippage_bps;
                    out.model_status = "evaluated".into();
                    stats.profitable += 1;
                } else {
                    out.attack_feasible = true;
                    out.model_status = "evaluated".into();
                    out.rejection_reason = Some("best_attempt_unprofitable".into());
                    stats.unprofitable += 1;
                }
            }
        }

        rows.push(out);
    }

    write_csv(out_csv, &rows)?;
    stats.output_rows = rows.len();
    Ok(stats)
}

#[derive(Default, Debug, Clone)]
pub struct EvaluateStats {
    pub input_rows: usize,
    pub output_rows: usize,
    pub below_threshold: usize,
    pub replay_mismatch: usize,
    pub invalid_state: usize,
    pub no_result: usize,
    pub profitable: usize,
    pub unprofitable: usize,
}

fn parse_mode(s: &str) -> CreatorFeeMode {
    match s.to_lowercase().as_str() {
        "oninput" | "on_input" => CreatorFeeMode::OnInput,
        "onoutput" | "on_output" => CreatorFeeMode::OnOutput,
        _ => CreatorFeeMode::Disabled,
    }
}

fn base_row(dec: &DecodedCandidateRow, tx_cost: u128, below: bool) -> CpmmAttackRow {
    CpmmAttackRow {
        pool_type: dec.pool_type.clone(),
        pool_label: dec.pool_label.clone(),
        pool_address: dec.pool_address.clone(),
        slot: dec.slot,
        signature: dec.signature.clone(),
        block_time: dec.block_time,
        instruction_index: dec.instruction_index,
        direction: dec.direction.clone(),
        amount_in: dec.amount_in,
        victim_amount_in: dec.amount_in,
        min_amount_out: dec.min_amount_out,
        actual_amount_out: dec.actual_amount_out,
        reserve_in_before: dec.reserve_in_before,
        reserve_out_before: dec.reserve_out_before,
        trade_fee_rate: dec.trade_fee_rate,
        creator_fee_rate: dec.creator_fee_rate,
        fee_denominator: dec.fee_denominator,
        creator_fee_mode: dec.creator_fee_mode.clone(),
        tx_cost_per_leg: tx_cost,
        model_version: MODEL_VERSION.into(),
        model_status: "evaluated".into(),
        rejection_reason: None,
        fair_amount_out: None,
        replay_error_bps: None,
        best_attempt_frontrun: 0,
        best_attempt_frontrun_output: 0,
        best_attempt_backrun_output: 0,
        best_attempt_gross_profit: 0,
        best_attempt_net_profit: 0,
        best_attempt_victim_extra_slippage_bps: 0,
        best_attempt_feasible: false,
        optimal_frontrun: 0,
        frontrun_output: 0,
        backrun_output: 0,
        attacker_gross_profit: 0,
        attacker_net_profit: 0,
        victim_extra_slippage_bps: 0,
        attack_feasible: false,
        attack_profitable: false,
        below_candidate_threshold: below,
    }
}
