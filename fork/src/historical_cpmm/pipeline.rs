use crate::historical_cpmm::artifacts::{
    read_signatures, top_rejection_reason, write_csv, DecodedCandidateRow, PipelineSummaryRow,
    SignatureRow, StatusRow,
};
use crate::historical_cpmm::config::PoolTarget;
use crate::historical_cpmm::decode::decode_target_swaps;
use crate::historical_cpmm::prestate::{build_candidate, StateCache};
use crate::historical_cpmm::signatures::collect_signatures;
use crate::historical_cpmm::status::AnalysisStatus;
use crate::historical_cpmm::transactions::{
    decode_transaction, fetch_and_cache_transaction, transaction_meta,
};
use anyhow::{Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CANDIDATE_THRESHOLD_BPS: u128 = 5;

#[derive(Clone, Debug)]
pub struct PipelinePaths {
    pub cache_root: PathBuf,
    pub results_dir: PathBuf,
    pub snapshot_root: PathBuf,
}

impl PipelinePaths {
    pub fn signatures_csv(&self) -> PathBuf {
        self.cache_root.join("signatures.csv")
    }

    pub fn status_csv(&self) -> PathBuf {
        self.results_dir.join("historical_cpmm_swaps_status.csv")
    }

    pub fn decoded_csv(&self) -> PathBuf {
        self.results_dir.join("historical_cpmm_decoded.csv")
    }

    pub fn summary_csv(&self) -> PathBuf {
        self.results_dir
            .join("historical_cpmm_pipeline_summary.csv")
    }
}

pub fn rpc_client(rpc_url: &str) -> RpcClient {
    RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::finalized())
}

pub fn collect_signatures_stage(
    rpc: &RpcClient,
    pools: &[PoolTarget],
    limit_per_pool: usize,
    paths: &PipelinePaths,
) -> Result<Vec<SignatureRow>> {
    let rows = collect_signatures(rpc, pools, limit_per_pool)?;
    write_csv(&paths.signatures_csv(), &rows)?;
    Ok(rows)
}

pub fn fetch_transactions_stage(
    rpc: &RpcClient,
    paths: &PipelinePaths,
) -> Result<Vec<SignatureRow>> {
    let rows = read_signatures(&paths.signatures_csv())?;
    for row in rows.iter().filter(|row| row.err.is_none()) {
        fetch_and_cache_transaction(rpc, &paths.cache_root, &row.signature)
            .with_context(|| format!("fetch/cache {}", row.signature))?;
    }
    Ok(rows)
}

pub fn build_decoded_stage(
    rpc: &RpcClient,
    paths: &PipelinePaths,
    tx_cost_per_leg: u128,
) -> Result<(
    Vec<DecodedCandidateRow>,
    Vec<StatusRow>,
    Vec<PipelineSummaryRow>,
)> {
    let signatures = read_signatures(&paths.signatures_csv())?;
    let mut decoded_rows = Vec::new();
    let mut status_rows = Vec::new();
    let mut state_cache = StateCache::new(paths.snapshot_root.clone());

    for sig in &signatures {
        if let Some(err) = &sig.err {
            status_rows.push(StatusRow::rejected(
                sig,
                AnalysisStatus::TxFailed,
                err.clone(),
            ));
            continue;
        }

        match process_signature(rpc, paths, &mut state_cache, sig, tx_cost_per_leg) {
            Ok((decoded, status)) => {
                decoded_rows.push(decoded);
                status_rows.push(status);
            }
            Err(status) => status_rows.push(status),
        }
    }

    let summary = summarize(&signatures, &decoded_rows, &status_rows);
    write_csv(&paths.decoded_csv(), &decoded_rows)?;
    write_csv(&paths.status_csv(), &status_rows)?;
    write_csv(&paths.summary_csv(), &summary)?;
    Ok((decoded_rows, status_rows, summary))
}

pub fn run_all(
    rpc: &RpcClient,
    pools: &[PoolTarget],
    limit_per_pool: usize,
    paths: &PipelinePaths,
    tx_cost_per_leg: u128,
) -> Result<(
    Vec<DecodedCandidateRow>,
    Vec<StatusRow>,
    Vec<PipelineSummaryRow>,
)> {
    collect_signatures_stage(rpc, pools, limit_per_pool, paths)?;
    fetch_transactions_stage(rpc, paths)?;
    build_decoded_stage(rpc, paths, tx_cost_per_leg)
}

fn process_signature(
    rpc: &RpcClient,
    paths: &PipelinePaths,
    state_cache: &mut StateCache,
    sig: &SignatureRow,
    tx_cost_per_leg: u128,
) -> Result<(DecodedCandidateRow, StatusRow), StatusRow> {
    let raw = fetch_and_cache_transaction(rpc, &paths.cache_root, &sig.signature)
        .map_err(|err| StatusRow::rejected(sig, AnalysisStatus::DecodeFailed, err.to_string()))?;
    let meta = transaction_meta(&raw).map_err(|err| {
        StatusRow::rejected(sig, AnalysisStatus::MissingPreState, err.to_string())
    })?;
    if let Some(err) = &meta.err {
        return Err(StatusRow::rejected(
            sig,
            AnalysisStatus::TxFailed,
            format!("{err:?}"),
        ));
    }
    let tx = decode_transaction(&raw)
        .map_err(|err| StatusRow::rejected(sig, AnalysisStatus::DecodeFailed, err.to_string()))?;

    let swaps = decode_target_swaps(&tx, meta, &sig.pool_address);
    if swaps.is_empty() {
        return Err(StatusRow::rejected(
            sig,
            AnalysisStatus::NoCpmmSwap,
            "no Raydium CPMM swap for target pool",
        ));
    }
    if swaps.len() > 1 {
        return Err(StatusRow::rejected(
            sig,
            AnalysisStatus::MultiSwapAmbiguous,
            format!("{} target swaps in one transaction", swaps.len()),
        ));
    }

    let swap = match swaps.into_iter().next().unwrap() {
        Ok(swap) => swap,
        Err(err) => {
            let reason = if err.to_string() == AnalysisStatus::UnsupportedSwapInstruction.as_str() {
                AnalysisStatus::UnsupportedSwapInstruction
            } else {
                AnalysisStatus::DecodeFailed
            };
            return Err(StatusRow::rejected(sig, reason, err.to_string()));
        }
    };

    let mut built = build_candidate(rpc, state_cache, sig, &tx, meta, &swap, tx_cost_per_leg)
        .map_err(|err| {
            StatusRow::rejected(sig, status_from_error(&err.to_string()), err.to_string())
        })?;

    if candidate_size_bps(&built.decoded) < CANDIDATE_THRESHOLD_BPS {
        built.status.analysis_status = AnalysisStatus::BelowCandidateThreshold.as_str().to_string();
        built.status.rejection_stage =
            Some(AnalysisStatus::BelowCandidateThreshold.stage().to_string());
        built.status.rejection_reason =
            Some(AnalysisStatus::BelowCandidateThreshold.as_str().to_string());
        built.status.rejection_detail =
            Some("decoded, but below analysis dust threshold".to_string());
    }

    Ok((built.decoded, built.status))
}

fn status_from_error(error: &str) -> AnalysisStatus {
    match error {
        "missing_pre_state" => AnalysisStatus::MissingPreState,
        "missing_token_balances" => AnalysisStatus::MissingTokenBalances,
        "multi_swap_ambiguous" => AnalysisStatus::MultiSwapAmbiguous,
        "vault_mismatch" => AnalysisStatus::VaultMismatch,
        "amount_mismatch" => AnalysisStatus::AmountMismatch,
        "invalid_fee_config" => AnalysisStatus::InvalidFeeConfig,
        "unsupported_swap_instruction" => AnalysisStatus::UnsupportedSwapInstruction,
        _ => AnalysisStatus::DecodeFailed,
    }
}

fn candidate_size_bps(row: &DecodedCandidateRow) -> u128 {
    if row.reserve_in_before == 0 {
        0
    } else {
        row.amount_in.saturating_mul(10_000) / row.reserve_in_before
    }
}

fn summarize(
    signatures: &[SignatureRow],
    decoded: &[DecodedCandidateRow],
    statuses: &[StatusRow],
) -> Vec<PipelineSummaryRow> {
    let created_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut by_pool = BTreeMap::<String, usize>::new();
    for sig in signatures {
        *by_pool.entry(sig.pool_label.clone()).or_default() += 1;
    }

    by_pool
        .into_iter()
        .flat_map(|(pool_label, input_rows)| {
            let output_rows = decoded
                .iter()
                .filter(|row| row.pool_label == pool_label)
                .count();
            let rejected_rows = statuses
                .iter()
                .filter(|row| row.pool_label == pool_label)
                .filter(|row| row.analysis_status != AnalysisStatus::Included.as_str())
                .count();
            [
                PipelineSummaryRow {
                    stage: "collect_signatures".to_string(),
                    pool_label: pool_label.clone(),
                    input_rows: 0,
                    output_rows: input_rows,
                    rejected_rows: 0,
                    top_rejection_reason: None,
                    created_at_unix,
                },
                PipelineSummaryRow {
                    stage: "build_decoded".to_string(),
                    pool_label: pool_label.clone(),
                    input_rows,
                    output_rows,
                    rejected_rows,
                    top_rejection_reason: top_rejection_reason(statuses, &pool_label),
                    created_at_unix,
                },
            ]
        })
        .collect()
}

pub fn default_paths(
    cache_root: impl Into<PathBuf>,
    results_dir: impl Into<PathBuf>,
    snapshot_root: impl Into<PathBuf>,
) -> PipelinePaths {
    PipelinePaths {
        cache_root: cache_root.into(),
        results_dir: results_dir.into(),
        snapshot_root: snapshot_root.into(),
    }
}

pub fn ensure_paths(paths: &PipelinePaths) -> Result<()> {
    std::fs::create_dir_all(&paths.cache_root)?;
    std::fs::create_dir_all(&paths.results_dir)?;
    Ok(())
}

pub fn signatures_path_exists(paths: &PipelinePaths) -> bool {
    Path::new(&paths.signatures_csv()).exists()
}
