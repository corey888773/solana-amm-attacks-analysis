use amm_math::multi_fee::{compute_swap_multi_fee, MultiFeeConfig};
use amm_math::sandwich::optimal_sandwich;
use amm_math::types::PoolState;
use amm_math::BPS_DENOMINATOR_F64;

use crate::config::{AttackerStrategy, SimConfig};
use crate::output::SimulationRecord;
use crate::scenarios::Scenario;

/// Lightweight swap result for the simulator's bookkeeping.
struct SwapStep {
    amount_out: u128,
    new_reserve_in: u128,
    new_reserve_out: u128,
    price_impact_bps: u64,
}

/// Apply a single CPMM swap using the multi-fee math
/// (`amm_math::multi_fee::compute_swap_multi_fee`) and return the post-swap
/// reserves alongside the price-impact bps. Returns `None` for any degenerate
/// state (zero output, output >= reserve_out, etc.) — same gating as the
/// legacy single-fee path.
fn apply_swap(
    reserve_in: u128,
    reserve_out: u128,
    amount_in: u128,
    cfg: &MultiFeeConfig,
) -> Option<SwapStep> {
    let amount_out = compute_swap_multi_fee(reserve_in, reserve_out, amount_in, cfg);
    if amount_out == 0 || amount_out >= reserve_out {
        return None;
    }
    let new_reserve_in = reserve_in.checked_add(amount_in)?;
    let new_reserve_out = reserve_out.checked_sub(amount_out)?;

    let price_before = reserve_out as f64 / reserve_in as f64;
    let price_after = new_reserve_out as f64 / new_reserve_in as f64;
    let price_impact_bps = ((1.0 - price_after / price_before) * BPS_DENOMINATOR_F64) as u64;

    Some(SwapStep {
        amount_out,
        new_reserve_in,
        new_reserve_out,
        price_impact_bps,
    })
}

/// Run a single scenario using pure math (Scenario A — no on-chain execution).
/// Returns one SimulationRecord per iteration.
pub fn run_scenario(scenario: &Scenario, config: &SimConfig) -> Vec<SimulationRecord> {
    let mut records = Vec::with_capacity(config.simulation.num_iterations as usize);

    for iteration in 0..config.simulation.num_iterations {
        if let Some(record) = run_single(scenario, config, iteration) {
            records.push(record);
        }
    }

    records
}

fn run_single(scenario: &Scenario, config: &SimConfig, iteration: u32) -> Option<SimulationRecord> {
    let pool = PoolState::new(
        scenario.pool_reserve_a,
        scenario.pool_reserve_b,
        scenario.pool_fee_bps,
    );
    let cfg = &scenario.fee_config;

    let r_in = pool.reserve_a as u128;
    let r_out = pool.reserve_b as u128;

    // Baseline: victim swap without attack.
    let fair_swap = apply_swap(r_in, r_out, scenario.victim_swap_amount as u128, cfg)?;

    // Determine frontrun amount based on strategy. The analytical optimum
    // (Zhou 2021, IEEE S&P) is bps-keyed and ignores the creator fee, so for
    // real-pool replays it is at best a starting heuristic — but it gives a
    // reasonable size and the actual profit is computed via the multi-fee
    // path below.
    let frontrun_amount = match config.attacker.strategy {
        AttackerStrategy::Optimal => {
            let analysis = optimal_sandwich(
                scenario.victim_swap_amount,
                pool.reserve_a,
                pool.reserve_b,
                pool.fee_bps,
                scenario.tx_cost,
            )?;
            analysis.frontrun_amount
        }
        AttackerStrategy::Fixed => config.attacker.fixed_frontrun_amount.unwrap_or(0),
    };

    if frontrun_amount == 0 {
        return None;
    }

    // Sandwich: frontrun -> victim -> backrun.
    let frontrun = apply_swap(r_in, r_out, frontrun_amount as u128, cfg)?;
    let victim_sandwiched = apply_swap(
        frontrun.new_reserve_in,
        frontrun.new_reserve_out,
        scenario.victim_swap_amount as u128,
        cfg,
    )?;
    // Backrun reverses direction: sell token_out for token_in.
    let backrun = apply_swap(
        victim_sandwiched.new_reserve_out,
        victim_sandwiched.new_reserve_in,
        frontrun.amount_out,
        cfg,
    )?;

    let backrun_out = backrun.amount_out.min(i64::MAX as u128) as i64;
    let gross_profit = backrun_out - frontrun_amount as i64;
    let net_profit = gross_profit - scenario.tx_cost as i64;

    let fair_out = fair_swap.amount_out.min(u64::MAX as u128) as u64;
    let sandwiched_out = victim_sandwiched.amount_out.min(u64::MAX as u128) as u64;
    let victim_loss = fair_out.saturating_sub(sandwiched_out);
    let victim_extra_slippage_bps = if fair_out > 0 {
        (victim_loss as f64 / fair_out as f64 * BPS_DENOMINATOR_F64) as u64
    } else {
        0
    };

    Some(SimulationRecord {
        pool_reserve_a: scenario.pool_reserve_a,
        pool_reserve_b: scenario.pool_reserve_b,
        pool_fee_bps: scenario.pool_fee_bps,
        trade_fee_rate: cfg.trade_fee_rate,
        creator_fee_rate: cfg.creator_fee_rate,
        fee_denominator: cfg.fee_denominator,
        victim_amount: scenario.victim_swap_amount,
        victim_slippage_tolerance_bps: scenario.victim_slippage_bps,
        frontrun_amount,
        attacker_gross_profit: gross_profit,
        attacker_net_profit: net_profit,
        attack_profitable: net_profit > 0,
        victim_amount_out_no_attack: fair_out,
        victim_amount_out_with_attack: sandwiched_out,
        victim_extra_slippage_bps,
        victim_loss_absolute: victim_loss,
        price_before: pool.spot_price(),
        price_after_attack: backrun.new_reserve_out as f64 / backrun.new_reserve_in as f64,
        price_impact_bps: frontrun.price_impact_bps + victim_sandwiched.price_impact_bps,
        tx_cost_total: scenario.tx_cost,
        iteration,
        source: "custom_amm".to_string(),
        pool_label: scenario.pool_label.clone(),
    })
}
