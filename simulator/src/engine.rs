use amm_math::constant_product::compute_swap;
use amm_math::sandwich::optimal_sandwich;
use amm_math::types::PoolState;

use crate::config::{AttackerStrategy, SimConfig};
use crate::output::SimulationRecord;
use crate::scenarios::Scenario;

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

    // Baseline: victim swap without attack
    let fair_swap = compute_swap(
        scenario.victim_swap_amount,
        pool.reserve_a,
        pool.reserve_b,
        pool.fee_bps,
    )?;

    // Determine frontrun amount based on strategy
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

    // Execute sandwich: frontrun -> victim -> backrun
    let frontrun = compute_swap(
        frontrun_amount,
        pool.reserve_a,
        pool.reserve_b,
        pool.fee_bps,
    )?;

    let victim_sandwiched = compute_swap(
        scenario.victim_swap_amount,
        frontrun.new_reserve_in,
        frontrun.new_reserve_out,
        pool.fee_bps,
    )?;

    // Backrun: sell token_out back to token_in (direction flips)
    let backrun = compute_swap(
        frontrun.amount_out,
        victim_sandwiched.new_reserve_out,
        victim_sandwiched.new_reserve_in,
        pool.fee_bps,
    )?;

    let gross_profit = backrun.amount_out as i64 - frontrun_amount as i64;
    let net_profit = gross_profit - scenario.tx_cost as i64;
    let victim_loss = fair_swap
        .amount_out
        .saturating_sub(victim_sandwiched.amount_out);
    let victim_extra_slippage_bps = if fair_swap.amount_out > 0 {
        (victim_loss as f64 / fair_swap.amount_out as f64 * amm_math::BPS_DENOMINATOR_F64) as u64
    } else {
        0
    };

    Some(SimulationRecord {
        pool_reserve_a: scenario.pool_reserve_a,
        pool_reserve_b: scenario.pool_reserve_b,
        pool_fee_bps: scenario.pool_fee_bps,
        victim_amount: scenario.victim_swap_amount,
        victim_slippage_tolerance_bps: scenario.victim_slippage_bps,
        frontrun_amount,
        attacker_gross_profit: gross_profit,
        attacker_net_profit: net_profit,
        attack_profitable: net_profit > 0,
        victim_amount_out_no_attack: fair_swap.amount_out,
        victim_amount_out_with_attack: victim_sandwiched.amount_out,
        victim_extra_slippage_bps,
        victim_loss_absolute: victim_loss,
        price_before: pool.spot_price(),
        price_after_attack: backrun.new_reserve_out as f64 / backrun.new_reserve_in as f64,
        price_impact_bps: frontrun.price_impact_bps + victim_sandwiched.price_impact_bps,
        tx_cost_total: scenario.tx_cost,
        iteration,
        source: "custom_amm".to_string(),
    })
}
