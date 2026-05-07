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
    pub frontrun_amount: u64,
    pub attacker_gross_profit: i64,
    pub attacker_net_profit: i64,
    pub attack_profitable: bool,
    pub attack_status: AttackStatus,

    // Victim results
    pub victim_amount_out_no_attack: u64,
    pub victim_amount_out_with_attack: u64,
    pub victim_extra_slippage_bps: u64,
    pub victim_loss_absolute: u64,

    // Pool state
    pub price_before: f64,
    pub price_after_attack: f64,
    pub price_impact_bps: u64,

    // Meta
    /// Total transaction cost for the sandwich (front+back) expressed in input-token units
    /// (smallest denomination of token_in), converted from lamports via
    /// `costs.input_token_per_sol`. CSV column name kept as `tx_cost_total` for backward compat.
    pub tx_cost_total: u64,
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
