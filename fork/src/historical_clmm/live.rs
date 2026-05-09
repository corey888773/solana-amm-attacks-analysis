use crate::historical_clmm::artifacts::DecodedObservationRow;
use crate::historical_clmm::config::PoolTarget;
use crate::historical_clmm::decode::decode_target_swaps;
use crate::historical_clmm::prestate::build_observation;
use crate::historical_cpmm::transactions::{
    decode_transaction, fetch_and_cache_transaction, transaction_meta,
};
use crate::programs::raydium_clmm_program_pubkey;
use crate::CachedAccount;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_clmm_decoder::accounts::pool_state::PoolState;
use csv::WriterBuilder;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct LiveCollectConfig {
    pub cache_root: PathBuf,
    pub results_dir: PathBuf,
    pub duration_seconds: u64,
    pub interval_seconds: u64,
    pub poll_limit: usize,
    pub tx_cost_per_leg: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveSnapshotRow {
    pub collected_at_unix: u64,
    pub pool_label: String,
    pub pool_address: String,
    pub account_pubkey: String,
    pub account_role: String,
    pub rpc_slot: Option<u64>,
    pub owner: Option<String>,
    pub data_len: Option<usize>,
    pub cache_path: Option<String>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveSwapRow {
    pub discovered_at_unix: u64,
    pub pool_label: String,
    pub pool_address: String,
    pub slot: u64,
    pub signature: String,
    pub block_time: Option<i64>,
    pub instruction_index: Option<u32>,
    pub swap_variant: Option<String>,
    pub direction: Option<String>,
    pub amount_in: Option<u128>,
    pub actual_amount_out: Option<u128>,
    pub tick_arrays: Option<String>,
    pub watch_accounts_known_before_discovery: usize,
    pub previous_snapshot_unix: Option<u64>,
    pub status: String,
    pub error: Option<String>,
    #[serde(default)]
    pub amount_specified: Option<u128>,
    #[serde(default)]
    pub min_amount_out: Option<u128>,
    #[serde(default)]
    pub sqrt_price_limit_x64: Option<u128>,
    #[serde(default)]
    pub is_base_input: Option<bool>,
    #[serde(default)]
    pub input_vault: Option<String>,
    #[serde(default)]
    pub output_vault: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveCandidateReadinessRow {
    pub pool_type: String,
    pub pool_label: String,
    pub pool_address: String,
    pub slot: u64,
    pub signature: String,
    pub block_time: Option<i64>,
    pub discovered_at_unix: u64,
    pub instruction_index: Option<u32>,
    pub direction: Option<String>,
    pub amount_specified: Option<u128>,
    pub amount_in: Option<u128>,
    pub min_amount_out: Option<u128>,
    pub actual_amount_out: Option<u128>,
    pub sqrt_price_limit_x64: Option<u128>,
    pub is_base_input: Option<bool>,
    #[serde(default)]
    pub input_vault: Option<String>,
    #[serde(default)]
    pub output_vault: Option<String>,
    pub previous_snapshot_unix: Option<u64>,
    pub snapshot_before_block_time: bool,
    pub required_tick_array_count: usize,
    pub snapshot_account_count: usize,
    pub pool_state_ready: bool,
    pub amm_config_ready: bool,
    pub observation_state_ready: bool,
    pub tick_arrays_ready: bool,
    pub live_candidate_ready: bool,
    pub rejection_reason: Option<String>,
    pub tick_arrays: Option<String>,
    pub missing_tick_arrays: Option<String>,
}

#[derive(Default)]
struct PoolWatch {
    accounts: BTreeMap<String, String>,
    last_snapshot_unix: Option<u64>,
}

pub fn run_live_collect(
    rpc: &RpcClient,
    pools: &[PoolTarget],
    cfg: LiveCollectConfig,
) -> Result<()> {
    std::fs::create_dir_all(&cfg.cache_root)?;
    std::fs::create_dir_all(&cfg.results_dir)?;

    let mut seen = read_seen_signatures(&live_swaps_csv(&cfg.results_dir))?;
    let mut watch = pools
        .iter()
        .map(|pool| {
            let mut pool_watch = PoolWatch::default();
            pool_watch
                .accounts
                .insert(pool.address.clone(), "pool_state".to_string());
            (pool.label.clone(), pool_watch)
        })
        .collect::<BTreeMap<_, _>>();

    let started_at = unix_now();
    let mut iteration = 0_u64;
    loop {
        iteration += 1;
        let now = unix_now();
        if now.saturating_sub(started_at) >= cfg.duration_seconds {
            break;
        }

        for pool in pools {
            if let Err(err) = bootstrap_pool_accounts(rpc, pool, &mut watch) {
                eprintln!("WARN bootstrap {}: {err:#}", pool.label);
            }

            let known_before = watch
                .get(&pool.label)
                .map(|pool_watch| pool_watch.accounts.len())
                .unwrap_or_default();
            let previous_snapshot_unix = watch.get(&pool.label).and_then(|w| w.last_snapshot_unix);

            if let Err(err) = poll_pool_swaps(
                rpc,
                pool,
                &cfg,
                &mut seen,
                &mut watch,
                known_before,
                previous_snapshot_unix,
            ) {
                eprintln!("WARN poll {}: {err:#}", pool.label);
            }

            if let Err(err) = snapshot_pool_watch(rpc, pool, &cfg, &mut watch) {
                eprintln!("WARN snapshot {}: {err:#}", pool.label);
            }
        }

        println!(
            "live-collect iteration={} elapsed={}s seen={} next_sleep={}s",
            iteration,
            unix_now().saturating_sub(started_at),
            seen.len(),
            cfg.interval_seconds
        );
        thread::sleep(Duration::from_secs(cfg.interval_seconds));
    }

    Ok(())
}

pub fn build_live_candidate_readiness(
    results_dir: &Path,
) -> Result<Vec<LiveCandidateReadinessRow>> {
    let swaps = read_live_swaps(&live_swaps_csv(results_dir))?;
    let snapshots = read_live_snapshots(&live_snapshots_csv(results_dir))?;
    let snapshot_index = index_snapshots(&snapshots);

    let rows = swaps
        .into_iter()
        .filter(|row| row.status == "decoded")
        .map(|swap| readiness_row(swap, &snapshot_index))
        .collect();

    Ok(rows)
}

pub fn live_candidates_csv(results_dir: &Path) -> PathBuf {
    results_dir.join("historical_clmm_live_candidates.csv")
}

fn bootstrap_pool_accounts(
    rpc: &RpcClient,
    pool: &PoolTarget,
    watch: &mut BTreeMap<String, PoolWatch>,
) -> Result<()> {
    let key = Pubkey::from_str(&pool.address)?;
    let response = rpc.get_account_with_commitment(&key, CommitmentConfig::finalized())?;
    let Some(account) = response.value else {
        return Ok(());
    };
    if account.owner != raydium_clmm_program_pubkey() {
        return Ok(());
    }
    let Some(pool_state) = <PoolState as CarbonDeserialize>::deserialize(&account.data) else {
        return Ok(());
    };

    let pool_watch = watch.entry(pool.label.clone()).or_default();
    pool_watch
        .accounts
        .insert(pool_state.amm_config.to_string(), "amm_config".to_string());
    pool_watch.accounts.insert(
        pool_state.observation_key.to_string(),
        "observation_state".to_string(),
    );
    Ok(())
}

fn poll_pool_swaps(
    rpc: &RpcClient,
    pool: &PoolTarget,
    cfg: &LiveCollectConfig,
    seen: &mut BTreeSet<String>,
    watch: &mut BTreeMap<String, PoolWatch>,
    known_before: usize,
    previous_snapshot_unix: Option<u64>,
) -> Result<()> {
    let pool_pubkey = pool.pubkey()?;
    let signatures = rpc.get_signatures_for_address_with_config(
        &pool_pubkey,
        GetConfirmedSignaturesForAddress2Config {
            before: None,
            until: None,
            limit: Some(cfg.poll_limit),
            commitment: Some(CommitmentConfig::finalized()),
        },
    )?;

    for sig in signatures.into_iter().rev() {
        let seen_key = format!("{}:{}", pool.label, sig.signature);
        if !seen.insert(seen_key) {
            continue;
        }

        let discovered_at_unix = unix_now();
        let mut row = LiveSwapRow {
            discovered_at_unix,
            pool_label: pool.label.clone(),
            pool_address: pool.address.clone(),
            slot: sig.slot,
            signature: sig.signature.clone(),
            block_time: sig.block_time,
            instruction_index: None,
            swap_variant: None,
            direction: None,
            amount_specified: None,
            min_amount_out: None,
            sqrt_price_limit_x64: None,
            is_base_input: None,
            input_vault: None,
            output_vault: None,
            amount_in: None,
            actual_amount_out: None,
            tick_arrays: None,
            watch_accounts_known_before_discovery: known_before,
            previous_snapshot_unix,
            status: "pending".to_string(),
            error: None,
        };

        if let Some(err) = sig.err {
            row.status = "tx_failed".to_string();
            row.error = Some(format!("{err:?}"));
            append_csv(&live_swaps_csv(&cfg.results_dir), &row)?;
            continue;
        }

        match decode_live_swap(rpc, pool, cfg, &sig.signature, discovered_at_unix, watch) {
            Ok(Some(decoded)) => {
                row.instruction_index = Some(decoded.instruction_index);
                row.swap_variant = Some(decoded.swap_variant.clone());
                row.direction = Some(decoded.direction.clone());
                row.amount_specified = Some(decoded.amount_specified);
                row.min_amount_out = decoded.min_amount_out;
                row.sqrt_price_limit_x64 = Some(decoded.sqrt_price_limit_x64);
                row.is_base_input = Some(decoded.is_base_input);
                row.input_vault = Some(decoded.input_vault.clone());
                row.output_vault = Some(decoded.output_vault.clone());
                row.amount_in = decoded.amount_in;
                row.actual_amount_out = decoded.actual_amount_out;
                row.tick_arrays = Some(decoded.tick_arrays.clone());
                row.status = "decoded".to_string();
            }
            Ok(None) => {
                row.status = "no_single_target_swap".to_string();
            }
            Err(err) => {
                row.status = "decode_failed".to_string();
                row.error = Some(format!("{err:#}"));
            }
        }
        append_csv(&live_swaps_csv(&cfg.results_dir), &row)?;
    }

    Ok(())
}

fn decode_live_swap(
    rpc: &RpcClient,
    pool: &PoolTarget,
    cfg: &LiveCollectConfig,
    signature: &str,
    discovered_at_unix: u64,
    watch: &mut BTreeMap<String, PoolWatch>,
) -> Result<Option<DecodedObservationRow>> {
    let raw = fetch_and_cache_transaction(rpc, &cfg.cache_root, signature)?;
    let meta = transaction_meta(&raw)?;
    let tx = decode_transaction(&raw)?;
    let swaps = decode_target_swaps(&tx, meta, &pool.address);
    if swaps.len() != 1 {
        return Ok(None);
    }

    let swap = swaps.into_iter().next().unwrap()?;
    let sig_row = crate::historical_clmm::artifacts::SignatureRow {
        pool_label: pool.label.clone(),
        pool_address: pool.address.clone(),
        signature: signature.to_string(),
        slot: raw.slot,
        block_time: raw.block_time,
        err: None,
        source_account: pool.address.clone(),
        collected_at_unix: discovered_at_unix,
    };
    let built = build_observation(&sig_row, &tx, meta, &swap, cfg.tx_cost_per_leg)?;

    let pool_watch = watch.entry(pool.label.clone()).or_default();
    pool_watch
        .accounts
        .insert(built.decoded.amm_config.clone(), "amm_config".to_string());
    pool_watch.accounts.insert(
        built.decoded.observation_state.clone(),
        "observation_state".to_string(),
    );
    for account in built
        .decoded
        .tick_arrays
        .split(';')
        .filter(|account| !account.is_empty())
    {
        pool_watch
            .accounts
            .entry(account.to_string())
            .or_insert_with(|| "remaining_tick_account".to_string());
    }

    Ok(Some(built.decoded))
}

fn snapshot_pool_watch(
    rpc: &RpcClient,
    pool: &PoolTarget,
    cfg: &LiveCollectConfig,
    watch: &mut BTreeMap<String, PoolWatch>,
) -> Result<()> {
    let collected_at_unix = unix_now();
    let accounts = watch
        .get(&pool.label)
        .map(|pool_watch| pool_watch.accounts.clone())
        .unwrap_or_default();

    for (account, role) in accounts {
        let row = snapshot_account(rpc, pool, cfg, collected_at_unix, &account, &role);
        append_csv(&live_snapshots_csv(&cfg.results_dir), &row)?;
    }

    watch
        .entry(pool.label.clone())
        .or_default()
        .last_snapshot_unix = Some(collected_at_unix);
    Ok(())
}

fn snapshot_account(
    rpc: &RpcClient,
    pool: &PoolTarget,
    cfg: &LiveCollectConfig,
    collected_at_unix: u64,
    account_pubkey: &str,
    account_role: &str,
) -> LiveSnapshotRow {
    let result = (|| -> Result<LiveSnapshotRow> {
        let key = Pubkey::from_str(account_pubkey)?;
        let response = rpc.get_account_with_commitment(&key, CommitmentConfig::finalized())?;
        let account = response.value.context("account not found")?;
        let cached = CachedAccount {
            pubkey: account_pubkey.to_string(),
            slot: response.context.slot,
            lamports: account.lamports,
            data_b64: STANDARD.encode(&account.data),
            owner: account.owner.to_string(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        };
        let path = live_account_path(
            &cfg.cache_root,
            &pool.label,
            collected_at_unix,
            account_pubkey,
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        cached.write(&path)?;
        Ok(LiveSnapshotRow {
            collected_at_unix,
            pool_label: pool.label.clone(),
            pool_address: pool.address.clone(),
            account_pubkey: account_pubkey.to_string(),
            account_role: account_role.to_string(),
            rpc_slot: Some(response.context.slot),
            owner: Some(account.owner.to_string()),
            data_len: Some(account.data.len()),
            cache_path: Some(path.to_string_lossy().to_string()),
            status: "ok".to_string(),
            error: None,
        })
    })();

    result.unwrap_or_else(|err| LiveSnapshotRow {
        collected_at_unix,
        pool_label: pool.label.clone(),
        pool_address: pool.address.clone(),
        account_pubkey: account_pubkey.to_string(),
        account_role: account_role.to_string(),
        rpc_slot: None,
        owner: None,
        data_len: None,
        cache_path: None,
        status: "error".to_string(),
        error: Some(format!("{err:#}")),
    })
}

fn append_csv<T: Serialize>(path: &Path, row: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let exists = path.exists() && path.metadata()?.len() > 0;
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut writer = WriterBuilder::new().has_headers(!exists).from_writer(file);
    writer.serialize(row)?;
    writer.flush()?;
    Ok(())
}

fn read_seen_signatures(path: &Path) -> Result<BTreeSet<String>> {
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let mut reader = csv::Reader::from_path(path)?;
    let mut seen = BTreeSet::new();
    for row in reader.deserialize::<LiveSwapRow>() {
        let row = row?;
        seen.insert(format!("{}:{}", row.pool_label, row.signature));
    }
    Ok(seen)
}

pub fn read_live_swaps(path: &Path) -> Result<Vec<LiveSwapRow>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut reader = csv::Reader::from_path(path)?;
    let mut rows = Vec::new();
    for row in reader.deserialize::<LiveSwapRow>() {
        rows.push(row?);
    }
    Ok(rows)
}

pub fn read_live_snapshots(path: &Path) -> Result<Vec<LiveSnapshotRow>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut reader = csv::Reader::from_path(path)?;
    let mut rows = Vec::new();
    for row in reader.deserialize::<LiveSnapshotRow>() {
        rows.push(row?);
    }
    Ok(rows)
}

#[derive(Default)]
struct SnapshotSet {
    account_count: usize,
    by_role: BTreeMap<String, usize>,
    accounts: BTreeSet<String>,
}

fn index_snapshots(rows: &[LiveSnapshotRow]) -> BTreeMap<(String, u64), SnapshotSet> {
    let mut index = BTreeMap::new();
    for row in rows.iter().filter(|row| row.status == "ok") {
        let set = index
            .entry((row.pool_label.clone(), row.collected_at_unix))
            .or_insert_with(SnapshotSet::default);
        set.account_count += 1;
        *set.by_role.entry(row.account_role.clone()).or_default() += 1;
        set.accounts.insert(row.account_pubkey.clone());
    }
    index
}

fn readiness_row(
    swap: LiveSwapRow,
    snapshot_index: &BTreeMap<(String, u64), SnapshotSet>,
) -> LiveCandidateReadinessRow {
    let tick_arrays = swap
        .tick_arrays
        .as_deref()
        .unwrap_or_default()
        .split(';')
        .filter(|account| !account.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    let snapshot = swap
        .previous_snapshot_unix
        .and_then(|snapshot_unix| snapshot_index.get(&(swap.pool_label.clone(), snapshot_unix)));

    let pool_state_ready = snapshot
        .and_then(|set| set.by_role.get("pool_state"))
        .copied()
        .unwrap_or_default()
        > 0;
    let amm_config_ready = snapshot
        .and_then(|set| set.by_role.get("amm_config"))
        .copied()
        .unwrap_or_default()
        > 0;
    let observation_state_ready = snapshot
        .and_then(|set| set.by_role.get("observation_state"))
        .copied()
        .unwrap_or_default()
        > 0;

    let missing_tick_arrays = tick_arrays
        .iter()
        .filter(|account| {
            snapshot
                .map(|set| !set.accounts.contains(*account))
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();
    let tick_arrays_ready = !tick_arrays.is_empty() && missing_tick_arrays.is_empty();
    let snapshot_account_count = snapshot.map(|set| set.account_count).unwrap_or_default();
    let snapshot_before_block_time = match (swap.previous_snapshot_unix, swap.block_time) {
        (Some(snapshot_unix), Some(block_time)) => snapshot_unix <= block_time.max(0) as u64,
        _ => false,
    };

    let rejection_reason = if swap.previous_snapshot_unix.is_none() {
        Some("missing_previous_snapshot".to_string())
    } else if swap.block_time.is_none() {
        Some("missing_block_time".to_string())
    } else if !snapshot_before_block_time {
        Some("previous_snapshot_after_block_time".to_string())
    } else if snapshot.is_none() {
        Some("previous_snapshot_not_found".to_string())
    } else if !pool_state_ready {
        Some("missing_pool_state_snapshot".to_string())
    } else if !amm_config_ready {
        Some("missing_amm_config_snapshot".to_string())
    } else if !observation_state_ready {
        Some("missing_observation_state_snapshot".to_string())
    } else if tick_arrays.is_empty() {
        Some("missing_tick_arrays_in_swap_row".to_string())
    } else if !tick_arrays_ready {
        Some("missing_tick_array_snapshot".to_string())
    } else {
        None
    };
    let live_candidate_ready = rejection_reason.is_none();

    LiveCandidateReadinessRow {
        pool_type: "raydium_clmm".to_string(),
        pool_label: swap.pool_label,
        pool_address: swap.pool_address,
        slot: swap.slot,
        signature: swap.signature,
        block_time: swap.block_time,
        discovered_at_unix: swap.discovered_at_unix,
        instruction_index: swap.instruction_index,
        direction: swap.direction,
        amount_specified: swap.amount_specified,
        amount_in: swap.amount_in,
        min_amount_out: swap.min_amount_out,
        actual_amount_out: swap.actual_amount_out,
        sqrt_price_limit_x64: swap.sqrt_price_limit_x64,
        is_base_input: swap.is_base_input,
        input_vault: swap.input_vault,
        output_vault: swap.output_vault,
        previous_snapshot_unix: swap.previous_snapshot_unix,
        snapshot_before_block_time,
        required_tick_array_count: tick_arrays.len(),
        snapshot_account_count,
        pool_state_ready,
        amm_config_ready,
        observation_state_ready,
        tick_arrays_ready,
        live_candidate_ready,
        rejection_reason,
        tick_arrays: (!tick_arrays.is_empty()).then(|| tick_arrays.join(";")),
        missing_tick_arrays: (!missing_tick_arrays.is_empty())
            .then(|| missing_tick_arrays.join(";")),
    }
}

fn live_swaps_csv(results_dir: &Path) -> PathBuf {
    results_dir.join("historical_clmm_live_swaps.csv")
}

fn live_snapshots_csv(results_dir: &Path) -> PathBuf {
    results_dir.join("historical_clmm_live_snapshots.csv")
}

fn live_account_path(
    cache_root: &Path,
    pool_label: &str,
    collected_at_unix: u64,
    account_pubkey: &str,
) -> PathBuf {
    cache_root
        .join("live_snapshots")
        .join(pool_label)
        .join(collected_at_unix.to_string())
        .join(format!("{account_pubkey}.json"))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded_swap(previous_snapshot_unix: Option<u64>, block_time: Option<i64>) -> LiveSwapRow {
        LiveSwapRow {
            discovered_at_unix: 1_010,
            pool_label: "pool".to_string(),
            pool_address: "pool_state".to_string(),
            slot: 42,
            signature: "sig".to_string(),
            block_time,
            instruction_index: Some(0),
            swap_variant: Some("swap".to_string()),
            direction: Some("base_input".to_string()),
            amount_specified: Some(100),
            min_amount_out: Some(80),
            sqrt_price_limit_x64: Some(0),
            is_base_input: Some(true),
            input_vault: Some("vault0".to_string()),
            output_vault: Some("vault1".to_string()),
            amount_in: Some(100),
            actual_amount_out: Some(90),
            tick_arrays: Some("tick_a;tick_b".to_string()),
            watch_accounts_known_before_discovery: 4,
            previous_snapshot_unix,
            status: "decoded".to_string(),
            error: None,
        }
    }

    fn ok_snapshot(
        account_pubkey: &str,
        account_role: &str,
        collected_at_unix: u64,
    ) -> LiveSnapshotRow {
        LiveSnapshotRow {
            collected_at_unix,
            pool_label: "pool".to_string(),
            pool_address: "pool_state".to_string(),
            account_pubkey: account_pubkey.to_string(),
            account_role: account_role.to_string(),
            rpc_slot: Some(1),
            owner: Some("owner".to_string()),
            data_len: Some(1),
            cache_path: Some("cache.json".to_string()),
            status: "ok".to_string(),
            error: None,
        }
    }

    #[test]
    fn readiness_rejects_snapshot_after_block_time() {
        let snapshots = vec![
            ok_snapshot("pool_state", "pool_state", 1_100),
            ok_snapshot("amm", "amm_config", 1_100),
            ok_snapshot("obs", "observation_state", 1_100),
            ok_snapshot("tick_a", "remaining_tick_account", 1_100),
            ok_snapshot("tick_b", "remaining_tick_account", 1_100),
        ];
        let index = index_snapshots(&snapshots);
        let row = readiness_row(decoded_swap(Some(1_100), Some(1_000)), &index);

        assert!(!row.live_candidate_ready);
        assert_eq!(
            row.rejection_reason.as_deref(),
            Some("previous_snapshot_after_block_time")
        );
    }

    #[test]
    fn readiness_accepts_complete_prior_snapshot() {
        let snapshots = vec![
            ok_snapshot("pool_state", "pool_state", 900),
            ok_snapshot("amm", "amm_config", 900),
            ok_snapshot("obs", "observation_state", 900),
            ok_snapshot("tick_a", "remaining_tick_account", 900),
            ok_snapshot("tick_b", "remaining_tick_account", 900),
        ];
        let index = index_snapshots(&snapshots);
        let row = readiness_row(decoded_swap(Some(900), Some(1_000)), &index);

        assert!(row.live_candidate_ready);
        assert!(row.rejection_reason.is_none());
    }
}
