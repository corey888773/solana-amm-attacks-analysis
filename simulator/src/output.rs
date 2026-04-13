use serde::Serialize;
use std::path::Path;

/// One row in the output CSV — covers both scenario A (custom AMM) and C (mainnet fork).
#[derive(Debug, Clone, Serialize)]
pub struct SimulationRecord {
    // Pool params
    pub pool_reserve_a: u64,
    pub pool_reserve_b: u64,
    pub pool_fee_bps: u16,

    // Victim params
    pub victim_amount: u64,
    pub victim_slippage_tolerance_bps: u16,

    // Attacker results
    pub frontrun_amount: u64,
    pub attacker_gross_profit: i64,
    pub attacker_net_profit: i64,
    pub attack_profitable: bool,

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
    pub tx_cost_total: u64,
    pub iteration: u32,
    pub source: String,
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
