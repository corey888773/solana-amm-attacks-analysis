use crate::historical_clmm::status::AnalysisStatus;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignatureRow {
    pub pool_label: String,
    pub pool_address: String,
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub err: Option<String>,
    pub source_account: String,
    pub collected_at_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusRow {
    pub pool_type: String,
    pub pool_label: String,
    pub pool_address: String,
    pub slot: u64,
    pub signature: String,
    pub block_time: Option<i64>,
    pub instruction_index: Option<u32>,
    pub swap_variant: Option<String>,
    pub direction: Option<String>,
    pub amount_in: Option<u128>,
    pub min_amount_out: Option<u128>,
    pub actual_amount_out: Option<u128>,
    pub analysis_status: String,
    pub rejection_stage: Option<String>,
    pub rejection_reason: Option<String>,
    pub rejection_detail: Option<String>,
}

impl StatusRow {
    pub fn rejected(sig: &SignatureRow, status: AnalysisStatus, detail: impl Into<String>) -> Self {
        Self {
            pool_type: "raydium_clmm".to_string(),
            pool_label: sig.pool_label.clone(),
            pool_address: sig.pool_address.clone(),
            slot: sig.slot,
            signature: sig.signature.clone(),
            block_time: sig.block_time,
            instruction_index: None,
            swap_variant: None,
            direction: None,
            amount_in: None,
            min_amount_out: None,
            actual_amount_out: None,
            analysis_status: status.as_str().to_string(),
            rejection_stage: Some(status.stage().to_string()),
            rejection_reason: Some(status.as_str().to_string()),
            rejection_detail: Some(detail.into()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecodedObservationRow {
    pub pool_type: String,
    pub pool_label: String,
    pub pool_address: String,
    pub slot: u64,
    pub signature: String,
    pub block_time: Option<i64>,
    pub instruction_index: u32,
    pub swap_variant: String,
    pub direction: String,
    pub is_base_input: bool,
    pub amount_specified: u128,
    pub other_amount_threshold: u128,
    pub sqrt_price_limit_x64: u128,
    pub amount_in: Option<u128>,
    pub min_amount_out: Option<u128>,
    pub actual_amount_out: Option<u128>,
    pub input_vault_before: Option<u128>,
    pub input_vault_after: Option<u128>,
    pub output_vault_before: Option<u128>,
    pub output_vault_after: Option<u128>,
    pub amm_config: String,
    pub input_vault: String,
    pub output_vault: String,
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub observation_state: String,
    pub tick_arrays: String,
    pub historical_state_status: String,
    pub tx_cost_per_leg: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelineSummaryRow {
    pub stage: String,
    pub pool_label: String,
    pub input_rows: usize,
    pub output_rows: usize,
    pub rejected_rows: usize,
    pub top_rejection_reason: Option<String>,
    pub created_at_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateProbeRow {
    pub pool_type: String,
    pub pool_label: String,
    pub pool_address: String,
    pub slot: u64,
    pub signature: String,
    pub instruction_index: u32,
    pub pool_state: String,
    pub amm_config: String,
    pub observation_state: String,
    pub tick_arrays: String,
    pub required_account_count: usize,
    pub current_probe_slot: Option<u64>,
    pub current_pool_state_ok: bool,
    pub current_amm_config_ok: bool,
    pub current_observation_state_found: bool,
    pub current_remaining_accounts_found: usize,
    pub current_remaining_accounts_missing: usize,
    pub current_tick_arrays_ok: usize,
    pub current_tick_array_decode_failures: usize,
    pub historical_state_available: bool,
    pub candidate_ready: bool,
    pub blocker: String,
}

pub fn read_signatures(path: &Path) -> Result<Vec<SignatureRow>> {
    let mut reader =
        csv::Reader::from_path(path).with_context(|| format!("read {}", path.display()))?;
    let mut rows = Vec::new();
    for row in reader.deserialize() {
        rows.push(row?);
    }
    Ok(rows)
}

pub fn read_decoded(path: &Path) -> Result<Vec<DecodedObservationRow>> {
    let mut reader =
        csv::Reader::from_path(path).with_context(|| format!("read {}", path.display()))?;
    let mut rows = Vec::new();
    for row in reader.deserialize() {
        rows.push(row?);
    }
    Ok(rows)
}

pub fn write_csv<T: Serialize>(path: &Path, rows: &[T]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let mut writer =
        csv::Writer::from_path(path).with_context(|| format!("write {}", path.display()))?;
    for row in rows {
        writer.serialize(row)?;
    }
    writer.flush()?;
    Ok(())
}

pub fn top_rejection_reason(rows: &[StatusRow], pool_label: &str) -> Option<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for row in rows
        .iter()
        .filter(|row| row.pool_label == pool_label)
        .filter_map(|row| row.rejection_reason.as_ref())
    {
        *counts.entry(row.clone()).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(reason, count)| format!("{} ({})", reason, count))
}
