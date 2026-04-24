use crate::config::{SimConfig, SweepParams};
use itertools::iproduct;

/// A single scenario to simulate (one combination of parameters).
#[derive(Debug, Clone)]
pub struct Scenario {
    pub pool_reserve_a: u64,
    pub pool_reserve_b: u64,
    pub pool_fee_bps: u16,
    pub victim_swap_amount: u64,
    pub victim_slippage_bps: u16,
    /// cost of 2 txs expressed in input-token units (converted from lamports at configured SOL price)
    pub tx_cost: u64,
}

/// Generate scenarios from config. If sweep defined, cartesian product of sweep values.
/// Otherwise single scenario from base config.
pub fn generate(config: &SimConfig) -> Vec<Scenario> {
    let base = Scenario {
        pool_reserve_a: config.pool.initial_reserve_a,
        pool_reserve_b: config.pool.initial_reserve_b,
        pool_fee_bps: config.pool.fee_bps,
        victim_swap_amount: config.victim.swap_amount,
        victim_slippage_bps: config.victim.slippage_tolerance_bps,
        tx_cost: config.costs.total_in_input_token(),
    };

    match &config.sweep {
        Some(sweep) => generate_sweep(&base, sweep),
        None => vec![base],
    }
}

fn generate_sweep(base: &Scenario, sweep: &SweepParams) -> Vec<Scenario> {
    let default_ra = [base.pool_reserve_a];
    let default_fee = [base.pool_fee_bps];
    let default_va = [base.victim_swap_amount];
    let default_slip = [base.victim_slippage_bps];

    let reserves_a = sweep.pool_reserve_a.as_deref().unwrap_or(&default_ra);
    let fees = sweep.pool_fee_bps.as_deref().unwrap_or(&default_fee);
    let victim_amounts = sweep.victim_swap_amount.as_deref().unwrap_or(&default_va);
    let slippages = sweep
        .victim_slippage_bps
        .as_deref()
        .unwrap_or(&default_slip);

    iproduct!(reserves_a, fees, victim_amounts, slippages)
        .map(|(&ra, &fee, &va, &slip)| {
            // Scale reserve_b proportionally to keep same initial price ratio
            let ratio = base.pool_reserve_b as f64 / base.pool_reserve_a as f64;
            let rb = (ra as f64 * ratio) as u64;

            Scenario {
                pool_reserve_a: ra,
                pool_reserve_b: rb,
                pool_fee_bps: fee,
                victim_swap_amount: va,
                victim_slippage_bps: slip,
                tx_cost: base.tx_cost,
            }
        })
        .collect()
}
