use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttackStatus {
    Executed,
    NoProfitableAttack,
    NoAttackConfigured,
}

/// One row in the output CSV — covers both scenario A (custom AMM) and C (mainnet fork).
#[derive(Debug, Clone, Serialize)]
pub struct SimulationRecord {
    // Pool params
    pub pool_reserve_a: u64,
    pub pool_reserve_b: u64,
    /// Lossy basis-points view of the trade fee, kept for backward
    /// compatibility with the synthetic-sweep notebooks. For real-pool
    /// replays prefer `trade_fee_rate` / `creator_fee_rate`.
    pub pool_fee_bps: u16,
    /// Trade-fee numerator (over `fee_denominator`). For synthetic sweeps,
    /// `pool_fee_bps` (denom 1e4); for real-pool replay, the on-chain CPMM
    /// trade fee numerator (denom 1e6).
    pub trade_fee_rate: u64,
    /// Creator-fee numerator (over `fee_denominator`). 0 for synthetic
    /// sweeps; copied from the snapshot's `AmmConfig` for real pools.
    pub creator_fee_rate: u64,
    /// Denominator for `trade_fee_rate` and `creator_fee_rate`. 10_000 for
    /// synthetic sweeps, 1_000_000 for Raydium CPMM real-pool replay.
    pub fee_denominator: u64,

    // Victim params
    pub victim_amount: u64,
    pub victim_slippage_tolerance_bps: u16,

    // Attacker results
    pub strategy: String,
    pub frontrun_amount: u64,
    pub attacker_gross_profit: i64,
    pub attacker_net_profit: i64,
    pub attack_profitable: bool,
    pub attack_feasible: bool,
    pub attack_status: AttackStatus,

    // Victim results
    pub victim_amount_out_no_attack: u64,
    pub victim_amount_out_with_attack: u64,
    pub victim_extra_slippage_bps: u64,
    pub victim_loss_absolute: u64,
    pub victim_reverted: bool,
    pub victim_size_bps_of_reserve: u64,
    pub frontrun_size_bps_of_reserve: u64,
    pub net_profit_bps_of_frontrun: i64,
    pub victim_loss_bps_of_fair_out: u64,

    // Pool state
    pub price_before: f64,
    pub price_after_attack: f64,
    pub price_impact_bps: u64,

    // Meta
    /// Cost of one attacker transaction leg expressed in input-token units
    /// (smallest denomination of token_in).
    pub tx_cost_per_leg: u64,
    /// Total transaction cost for the sandwich (front+back) expressed in
    /// input-token units.
    pub tx_cost_total: u64,
    pub tx_cost_per_leg_lamports: u64,
    pub tx_cost_total_lamports: u64,
    pub iteration: u32,
    pub source: String,
    /// Snapshot label when the run replays a real pool, "synthetic" otherwise.
    pub pool_label: String,
}

/// Write a batch of records to CSV.
pub fn write_csv(
    records: &[SimulationRecord],
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut wtr = csv::Writer::from_path(path)?;
    for r in records {
        wtr.serialize(r)?;
    }
    wtr.flush()?;
    Ok(())
}
