use amm_math::multi_fee::{compute_swap_multi_fee, MultiFeeConfig};
use amm_math::sandwich::closed_form::compute_closed_form_sandwich;
use amm_math::sandwich::numerical::compute_numerical_sandwich;
use amm_math::types::{PoolState, SwapResult};
use amm_math::BPS_DENOMINATOR_F64;

use crate::config::{AttackerStrategy, SimConfig};
use crate::output::{AttackStatus, SimulationRecord};
use crate::scenarios::Scenario;

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
) -> Option<SwapResult> {
    compute_swap_multi_fee(reserve_in, reserve_out, amount_in, cfg)
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

fn no_attack_record(
    scenario: &Scenario,
    cfg: &MultiFeeConfig,
    pool: &PoolState,
    fair_swap: &SwapResult,
    iteration: u32,
    status: AttackStatus,
) -> SimulationRecord {
    let fair_out = fair_swap.amount_out.min(u64::MAX as u128) as u64;
    let price = pool.spot_price();

    SimulationRecord {
        pool_reserve_a: scenario.pool_reserve_a,
        pool_reserve_b: scenario.pool_reserve_b,
        pool_fee_bps: scenario.pool_fee_bps,
        trade_fee_rate: cfg.trade_fee_rate,
        creator_fee_rate: cfg.creator_fee_rate,
        fee_denominator: cfg.fee_denominator,
        victim_amount: scenario.victim_swap_amount,
        victim_slippage_tolerance_bps: scenario.victim_slippage_bps,
        frontrun_amount: 0,
        attacker_gross_profit: 0,
        attacker_net_profit: 0,
        attack_profitable: false,
        attack_status: status,
        victim_amount_out_no_attack: fair_out,
        victim_amount_out_with_attack: fair_out,
        victim_extra_slippage_bps: 0,
        victim_loss_absolute: 0,
        price_before: price,
        price_after_attack: price,
        price_impact_bps: 0,
        tx_cost_total: scenario.tx_cost,
        iteration,
        source: "custom_amm".to_string(),
        pool_label: scenario.pool_label.clone(),
    }
}

fn run_single(scenario: &Scenario, config: &SimConfig, iteration: u32) -> Option<SimulationRecord> {
    let pool = PoolState::new(
        scenario.pool_reserve_a as u128,
        scenario.pool_reserve_b as u128,
        scenario.pool_fee_bps,
    );
    let cfg = &scenario.fee_config;

    let r_in = pool.reserve_a;
    let r_out = pool.reserve_b;

    // Baseline: victim swap without attack.
    let fair_swap = apply_swap(r_in, r_out, scenario.victim_swap_amount as u128, cfg)?;

    // Determine frontrun amount based on strategy. Closed-form is the Zhou
    // baseline; numerical uses the same multi-fee swap model as the simulator.
    let frontrun_amount = match config.attacker.strategy {
        AttackerStrategy::ClosedForm => match compute_closed_form_sandwich(
            scenario.victim_swap_amount as u128,
            pool.reserve_a,
            pool.reserve_b,
            pool.fee_bps,
            scenario.tx_cost as u128,
        ) {
            Some(analysis) => analysis.frontrun_amount.min(u64::MAX as u128) as u64,
            None => {
                return Some(no_attack_record(
                    scenario,
                    cfg,
                    &pool,
                    &fair_swap,
                    iteration,
                    AttackStatus::NoProfitableAttack,
                ));
            }
        },
        AttackerStrategy::Numerical => match compute_numerical_sandwich(
            pool.reserve_a,
            pool.reserve_b,
            scenario.victim_swap_amount as u128,
            cfg,
            scenario.tx_cost as u128,
        ) {
            Some(analysis) => analysis.frontrun_amount.min(u64::MAX as u128) as u64,
            None => {
                return Some(no_attack_record(
                    scenario,
                    cfg,
                    &pool,
                    &fair_swap,
                    iteration,
                    AttackStatus::NoProfitableAttack,
                ));
            }
        },
        AttackerStrategy::Fixed => match config.attacker.fixed_frontrun_amount {
            Some(amount) if amount > 0 => amount,
            _ => {
                return Some(no_attack_record(
                    scenario,
                    cfg,
                    &pool,
                    &fair_swap,
                    iteration,
                    AttackStatus::NoAttackConfigured,
                ));
            }
        },
    };

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
        attack_status: AttackStatus::Executed,
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

#[cfg(test)]
mod tests {
    use super::*;
    use amm_math::multi_fee::CreatorFeeMode;

    fn cfg(strategy: AttackerStrategy, fixed_frontrun_amount: Option<u64>) -> SimConfig {
        SimConfig {
            simulation: crate::config::SimulationParams {
                num_iterations: 1,
                seed: 42,
            },
            pool: crate::config::PoolParams {
                initial_reserve_a: 1_000_000,
                initial_reserve_b: 1_000_000,
                fee_bps: 30,
            },
            victim: crate::config::VictimParams {
                swap_amount: 10,
                slippage_tolerance_bps: 300,
                direction: crate::config::SwapDirection::AToB,
            },
            costs: crate::config::CostParams {
                base_fee_lamports: 0,
                priority_fee_lamports: 0,
                jito_tip_lamports: 0,
                input_token_per_sol: 1.0,
            },
            attacker: crate::config::AttackerParams {
                strategy,
                fixed_frontrun_amount,
            },
            sweep: None,
            real_pool: None,
        }
    }

    fn scenario(tx_cost: u64) -> Scenario {
        Scenario {
            pool_reserve_a: 1_000_000,
            pool_reserve_b: 1_000_000,
            pool_fee_bps: 30,
            fee_config: MultiFeeConfig {
                trade_fee_rate: 30,
                creator_fee_rate: 0,
                fee_denominator: 10_000,
                creator_fee_mode: CreatorFeeMode::Disabled,
            },
            victim_swap_amount: 10,
            victim_slippage_bps: 300,
            tx_cost,
            pool_label: "synthetic".to_string(),
        }
    }

    #[test]
    fn numerical_no_profitable_attack_still_records_scenario() {
        let records = run_scenario(
            &scenario(1_000_000_000),
            &cfg(AttackerStrategy::Numerical, None),
        );

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].attack_status, AttackStatus::NoProfitableAttack);
        assert_eq!(records[0].frontrun_amount, 0);
        assert!(!records[0].attack_profitable);
        assert_eq!(
            records[0].victim_amount_out_no_attack,
            records[0].victim_amount_out_with_attack
        );
    }

    #[test]
    fn fixed_zero_amount_records_no_attack_configured() {
        let records = run_scenario(&scenario(0), &cfg(AttackerStrategy::Fixed, Some(0)));

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].attack_status, AttackStatus::NoAttackConfigured);
        assert_eq!(records[0].frontrun_amount, 0);
        assert_eq!(records[0].attacker_net_profit, 0);
    }
}
