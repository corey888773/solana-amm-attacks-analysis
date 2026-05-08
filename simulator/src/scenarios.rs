use crate::config::{SimConfig, SweepParams};
use crate::real_pool::{fee_config_from_pool, load_real_pool, RealPool};
use amm_math::multi_fee::MultiFeeConfig;
use itertools::iproduct;
use std::path::Path;

/// A single scenario to simulate (one combination of parameters).
#[derive(Debug, Clone)]
pub struct Scenario {
    pub pool_reserve_a: u64,
    pub pool_reserve_b: u64,
    /// Approximate basis-points fee. For synthetic sweeps this is the source
    /// of truth; for real-pool replay it is a lossy projection of
    /// `trade_fee_rate` (denom=1e6) and is used only by the bps-keyed
    /// analytical closed-form baseline (`amm_math::sandwich::closed_form`).
    pub pool_fee_bps: u16,
    /// Multi-component fee schedule used by the actual swap math
    /// (`amm_math::multi_fee::compute_swap_multi_fee`).
    pub fee_config: MultiFeeConfig,
    pub victim_swap_amount: u64,
    pub victim_slippage_bps: u16,
    /// Cost of one attacker transaction leg, expressed in input-token units.
    /// Passed to optimizers that internally account for frontrun + backrun.
    pub tx_cost_per_leg: u64,
    /// Total sandwich transaction cost (frontrun + backrun), expressed in
    /// input-token units. Used for final CSV net-profit accounting.
    pub tx_cost_total: u64,
    pub tx_cost_per_leg_lamports: u64,
    pub tx_cost_total_lamports: u64,
    /// Snapshot label when replaying a real pool, "synthetic" otherwise.
    pub pool_label: String,
}

/// Generate scenarios from config. If sweep defined, cartesian product of sweep values.
/// Otherwise single scenario from base config.
pub fn generate(config: &SimConfig) -> Result<Vec<Scenario>, String> {
    // If a real-pool snapshot is configured, override pool_x/pool_y/fee with
    // values from the cached snapshot. The remaining sweep dims (victim_size,
    // slippage_bps) still vary normally.
    let (base_reserve_a, base_reserve_b, base_fee_bps, pool_label, force_pool, real_pool) =
        match &config.real_pool {
            Some(rp) => match load_real_pool(Path::new(&rp.manifest)) {
                Ok(p) => {
                    eprintln!(
                        "real_pool: label={} reserve_a={} reserve_b={} trade_fee_rate={} creator_fee_rate={}",
                        p.label, p.reserve_a, p.reserve_b, p.trade_fee_rate, p.creator_fee_rate
                    );
                    // Saturating cast: u128 reserves only exceed u64 for
                    // pathological pools; CPMM enforces u64 vault amounts.
                    let ra = p.reserve_a.min(u64::MAX as u128) as u64;
                    let rb = p.reserve_b.min(u64::MAX as u128) as u64;
                    let fee_bps = p.pool_fee_bps_approx();
                    let label = p.label.clone();
                    (ra, rb, fee_bps, label, true, Some(p))
                }
                Err(e) => {
                    if !rp.allow_synthetic_fallback {
                        return Err(format!(
                            "real_pool: failed to load snapshot at {} ({}); set real_pool.allow_synthetic_fallback = true to run synthetic fallback",
                            rp.manifest, e
                        ));
                    }
                    eprintln!(
                        "real_pool: failed to load snapshot at {} ({}); falling back to synthetic config because allow_synthetic_fallback=true",
                        rp.manifest, e
                    );
                    (
                        config.pool.initial_reserve_a,
                        config.pool.initial_reserve_b,
                        config.pool.fee_bps,
                        "synthetic".to_string(),
                        false,
                        None,
                    )
                }
            },
            None => (
                config.pool.initial_reserve_a,
                config.pool.initial_reserve_b,
                config.pool.fee_bps,
                "synthetic".to_string(),
                false,
                None,
            ),
        };

    let base = Scenario {
        pool_reserve_a: base_reserve_a,
        pool_reserve_b: base_reserve_b,
        pool_fee_bps: base_fee_bps,
        fee_config: fee_config_from_pool(base_fee_bps, real_pool.as_ref()),
        victim_swap_amount: config.victim.swap_amount,
        victim_slippage_bps: config.victim.slippage_tolerance_bps,
        tx_cost_per_leg: config.costs.per_leg_in_input_token(),
        tx_cost_total: config.costs.total_sandwich_in_input_token(),
        tx_cost_per_leg_lamports: config.costs.per_leg_lamports(),
        tx_cost_total_lamports: config.costs.total_sandwich_lamports(),
        pool_label,
    };

    Ok(match &config.sweep {
        Some(sweep) => generate_sweep(&base, sweep, force_pool, real_pool.as_ref()),
        None => vec![base],
    })
}

fn generate_sweep(
    base: &Scenario,
    sweep: &SweepParams,
    force_pool: bool,
    real_pool: Option<&RealPool>,
) -> Vec<Scenario> {
    let default_ra = [base.pool_reserve_a];
    let default_fee = [base.pool_fee_bps];
    let default_va = [base.victim_swap_amount];
    let default_slip = [base.victim_slippage_bps];

    // When a real pool is forced, ignore the synthetic reserve / fee grid:
    // the whole point of real-pool replay is to fix those to the snapshot.
    let reserves_a: &[u64] = if force_pool {
        &default_ra
    } else {
        sweep.pool_reserve_a.as_deref().unwrap_or(&default_ra)
    };
    let fees: &[u16] = if force_pool {
        &default_fee
    } else {
        sweep.pool_fee_bps.as_deref().unwrap_or(&default_fee)
    };
    let victim_amounts = sweep.victim_swap_amount.as_deref().unwrap_or(&default_va);
    let slippages = sweep
        .victim_slippage_bps
        .as_deref()
        .unwrap_or(&default_slip);

    iproduct!(reserves_a, fees, victim_amounts, slippages)
        .map(|(&ra, &fee, &va, &slip)| {
            // Scale reserve_b proportionally to keep same initial price ratio.
            // For real-pool replay, ra == base.pool_reserve_a so rb == base.pool_reserve_b.
            let ratio = base.pool_reserve_b as f64 / base.pool_reserve_a as f64;
            let rb = (ra as f64 * ratio) as u64;

            Scenario {
                pool_reserve_a: ra,
                pool_reserve_b: rb,
                pool_fee_bps: fee,
                fee_config: fee_config_from_pool(fee, real_pool),
                victim_swap_amount: va,
                victim_slippage_bps: slip,
                tx_cost_per_leg: base.tx_cost_per_leg,
                tx_cost_total: base.tx_cost_total,
                tx_cost_per_leg_lamports: base.tx_cost_per_leg_lamports,
                tx_cost_total_lamports: base.tx_cost_total_lamports,
                pool_label: base.pool_label.clone(),
            }
        })
        .collect()
}
