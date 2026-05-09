use crate::historical_clmm::artifacts::write_csv;
use crate::historical_clmm::live::{
    live_candidates_csv, read_live_snapshots, LiveCandidateReadinessRow, LiveSnapshotRow,
};
use crate::CachedAccount;
use anyhow::{anyhow, Context, Result};
use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_clmm_decoder::accounts::{amm_config::AmmConfig, pool_state::PoolState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FEE_DENOMINATOR: f64 = 1_000_000.0;
const Q64: f64 = 18_446_744_073_709_551_616.0;

#[derive(Clone, Debug)]
pub struct LiveAttackConfig {
    pub results_dir: PathBuf,
    pub max_steps: u64,
    pub replay_tolerance_bps: u64,
    pub tx_cost_per_leg: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveClmmAttackRow {
    pub pool_type: String,
    pub pool_label: String,
    pub pool_address: String,
    pub slot: u64,
    pub signature: String,
    pub block_time: Option<i64>,
    pub instruction_index: Option<u32>,
    pub direction: Option<String>,
    pub amount_in: Option<u128>,
    pub min_amount_out: Option<u128>,
    pub actual_amount_out: Option<u128>,
    pub previous_snapshot_unix: Option<u64>,
    pub snapshot_before_block_time: bool,
    pub live_candidate_ready: bool,
    pub model_version: String,
    pub model_status: String,
    pub rejection_reason: Option<String>,
    pub pool_sqrt_price_x64_before: Option<u128>,
    pub pool_liquidity_before: Option<u128>,
    pub pool_tick_current_before: Option<i32>,
    pub trade_fee_rate: Option<u32>,
    pub tick_spacing: Option<u16>,
    pub fair_amount_out: Option<u128>,
    pub replay_error_bps: Option<u64>,
    pub optimal_frontrun: u128,
    pub frontrun_output: u128,
    pub backrun_output: u128,
    pub attacker_gross_profit: i128,
    pub attacker_net_profit: i128,
    pub victim_loss_absolute: u128,
    pub victim_extra_slippage_bps: u64,
    pub attack_feasible: bool,
    pub attack_profitable: bool,
    pub tx_cost_per_leg: u128,
}

#[derive(Clone)]
struct AccountSnapshotIndex {
    by_role: BTreeMap<(String, u64, String), LiveSnapshotRow>,
    by_pubkey: BTreeMap<(String, u64, String), LiveSnapshotRow>,
}

#[derive(Clone, Copy, Debug)]
struct LocalClmmState {
    sqrt_price: f64,
    liquidity: f64,
    trade_fee_rate: f64,
}

#[derive(Clone, Copy, Debug)]
struct LocalSwapResult {
    amount_out: u128,
    next_state: LocalClmmState,
}

#[derive(Clone, Copy, Debug)]
struct LocalSandwichResult {
    frontrun_amount: u128,
    frontrun_output: u128,
    backrun_output: u128,
    gross_profit: i128,
    net_profit: i128,
    victim_loss_absolute: u128,
    victim_extra_slippage_bps: u64,
    attack_feasible: bool,
    attack_profitable: bool,
}

pub fn evaluate_live_attacks(cfg: &LiveAttackConfig) -> Result<Vec<LiveClmmAttackRow>> {
    let candidates = read_live_candidates(&live_candidates_csv(&cfg.results_dir))?;
    let snapshots =
        read_live_snapshots(&cfg.results_dir.join("historical_clmm_live_snapshots.csv"))?;
    let snapshot_index = AccountSnapshotIndex::new(snapshots);

    let rows = candidates
        .into_iter()
        .map(|candidate| evaluate_candidate(candidate, &snapshot_index, cfg))
        .collect::<Vec<_>>();

    write_csv(
        &cfg.results_dir.join("historical_clmm_candidates.csv"),
        &rows,
    )?;
    Ok(rows)
}

fn read_live_candidates(path: &Path) -> Result<Vec<LiveCandidateReadinessRow>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut reader = csv::Reader::from_path(path)?;
    let mut rows = Vec::new();
    for row in reader.deserialize::<LiveCandidateReadinessRow>() {
        rows.push(row?);
    }
    Ok(rows)
}

impl AccountSnapshotIndex {
    fn new(rows: Vec<LiveSnapshotRow>) -> Self {
        let mut by_role = BTreeMap::new();
        let mut by_pubkey = BTreeMap::new();
        for row in rows.into_iter().filter(|row| row.status == "ok") {
            by_role.insert(
                (
                    row.pool_label.clone(),
                    row.collected_at_unix,
                    row.account_role.clone(),
                ),
                row.clone(),
            );
            by_pubkey.insert(
                (
                    row.pool_label.clone(),
                    row.collected_at_unix,
                    row.account_pubkey.clone(),
                ),
                row,
            );
        }
        Self { by_role, by_pubkey }
    }

    fn role(&self, pool_label: &str, snapshot_unix: u64, role: &str) -> Option<&LiveSnapshotRow> {
        self.by_role
            .get(&(pool_label.to_string(), snapshot_unix, role.to_string()))
    }

    fn pubkey(
        &self,
        pool_label: &str,
        snapshot_unix: u64,
        account: &str,
    ) -> Option<&LiveSnapshotRow> {
        self.by_pubkey
            .get(&(pool_label.to_string(), snapshot_unix, account.to_string()))
    }
}

fn evaluate_candidate(
    candidate: LiveCandidateReadinessRow,
    snapshots: &AccountSnapshotIndex,
    cfg: &LiveAttackConfig,
) -> LiveClmmAttackRow {
    match evaluate_candidate_inner(&candidate, snapshots, cfg) {
        Ok(row) => row,
        Err(err) => base_row(
            candidate,
            cfg,
            "rejected",
            Some(format!("evaluation_error: {err:#}")),
        ),
    }
}

fn evaluate_candidate_inner(
    candidate: &LiveCandidateReadinessRow,
    snapshots: &AccountSnapshotIndex,
    cfg: &LiveAttackConfig,
) -> Result<LiveClmmAttackRow> {
    if !candidate.live_candidate_ready {
        return Ok(base_row(
            candidate.clone(),
            cfg,
            "rejected",
            candidate.rejection_reason.clone(),
        ));
    }
    if candidate.is_base_input != Some(true) {
        return Ok(base_row(
            candidate.clone(),
            cfg,
            "rejected",
            Some("unsupported_base_output".to_string()),
        ));
    }

    let amount_in = candidate
        .amount_in
        .ok_or_else(|| anyhow!("missing amount_in"))?;
    let actual_amount_out = candidate
        .actual_amount_out
        .ok_or_else(|| anyhow!("missing actual_amount_out"))?;
    let snapshot_unix = candidate
        .previous_snapshot_unix
        .ok_or_else(|| anyhow!("missing previous snapshot"))?;

    let pool_state = load_pool_state(
        snapshots
            .pubkey(
                &candidate.pool_label,
                snapshot_unix,
                &candidate.pool_address,
            )
            .or_else(|| snapshots.role(&candidate.pool_label, snapshot_unix, "pool_state"))
            .context("pool_state snapshot not found")?,
    )?;
    let amm_config = load_amm_config(
        snapshots
            .pubkey(
                &candidate.pool_label,
                snapshot_unix,
                &pool_state.amm_config.to_string(),
            )
            .or_else(|| snapshots.role(&candidate.pool_label, snapshot_unix, "amm_config"))
            .context("amm_config snapshot not found")?,
    )?;

    let zero_for_one = infer_zero_for_one(candidate, &pool_state)?;
    let tick_arrays_valid = validate_tick_arrays(candidate, snapshots, snapshot_unix)?;
    if !tick_arrays_valid {
        return Ok(base_row(
            candidate.clone(),
            cfg,
            "rejected",
            Some("tick_array_snapshot_decode_failed".to_string()),
        ));
    }

    let state = LocalClmmState {
        sqrt_price: pool_state.sqrt_price_x64 as f64 / Q64,
        liquidity: pool_state.liquidity as f64,
        trade_fee_rate: amm_config.trade_fee_rate as f64,
    };

    // CLMM active-range formulas are from Uniswap v3 Core whitepaper
    // (Adams et al., 2021), §6.2.1-§6.2.3. This first evaluator intentionally
    // rejects rows whose victim-only replay does not match observed output.
    let fair_swap = local_base_input_swap(state, amount_in, zero_for_one)
        .ok_or_else(|| anyhow!("local victim swap failed"))?;
    let fair_amount_out = fair_swap.amount_out;
    let replay_error_bps = bps_diff(fair_amount_out, actual_amount_out);
    if replay_error_bps > cfg.replay_tolerance_bps {
        let mut row = base_row(
            candidate.clone(),
            cfg,
            "rejected",
            Some("victim_replay_mismatch".to_string()),
        );
        row.pool_sqrt_price_x64_before = Some(pool_state.sqrt_price_x64);
        row.pool_liquidity_before = Some(pool_state.liquidity);
        row.pool_tick_current_before = Some(pool_state.tick_current);
        row.trade_fee_rate = Some(amm_config.trade_fee_rate);
        row.tick_spacing = Some(amm_config.tick_spacing);
        row.fair_amount_out = Some(fair_amount_out);
        row.replay_error_bps = Some(replay_error_bps);
        return Ok(row);
    }

    let sandwich = grid_sandwich(
        state,
        amount_in,
        candidate.min_amount_out,
        zero_for_one,
        cfg.tx_cost_per_leg,
        cfg.max_steps,
    );

    let mut row = base_row(candidate.clone(), cfg, "evaluated", None);
    row.pool_sqrt_price_x64_before = Some(pool_state.sqrt_price_x64);
    row.pool_liquidity_before = Some(pool_state.liquidity);
    row.pool_tick_current_before = Some(pool_state.tick_current);
    row.trade_fee_rate = Some(amm_config.trade_fee_rate);
    row.tick_spacing = Some(amm_config.tick_spacing);
    row.fair_amount_out = Some(fair_amount_out);
    row.replay_error_bps = Some(replay_error_bps);
    if let Some(result) = sandwich {
        row.optimal_frontrun = result.frontrun_amount;
        row.frontrun_output = result.frontrun_output;
        row.backrun_output = result.backrun_output;
        row.attacker_gross_profit = result.gross_profit;
        row.attacker_net_profit = result.net_profit;
        row.victim_loss_absolute = result.victim_loss_absolute;
        row.victim_extra_slippage_bps = result.victim_extra_slippage_bps;
        row.attack_feasible = result.attack_feasible;
        row.attack_profitable = result.attack_profitable;
    } else {
        row.rejection_reason = Some("no_profitable_attack".to_string());
    }
    Ok(row)
}

fn base_row(
    candidate: LiveCandidateReadinessRow,
    cfg: &LiveAttackConfig,
    model_status: &str,
    rejection_reason: Option<String>,
) -> LiveClmmAttackRow {
    LiveClmmAttackRow {
        pool_type: candidate.pool_type,
        pool_label: candidate.pool_label,
        pool_address: candidate.pool_address,
        slot: candidate.slot,
        signature: candidate.signature,
        block_time: candidate.block_time,
        instruction_index: candidate.instruction_index,
        direction: candidate.direction,
        amount_in: candidate.amount_in,
        min_amount_out: candidate.min_amount_out,
        actual_amount_out: candidate.actual_amount_out,
        previous_snapshot_unix: candidate.previous_snapshot_unix,
        snapshot_before_block_time: candidate.snapshot_before_block_time,
        live_candidate_ready: candidate.live_candidate_ready,
        model_version: "local_active_liquidity_v0_float".to_string(),
        model_status: model_status.to_string(),
        rejection_reason,
        pool_sqrt_price_x64_before: None,
        pool_liquidity_before: None,
        pool_tick_current_before: None,
        trade_fee_rate: None,
        tick_spacing: None,
        fair_amount_out: None,
        replay_error_bps: None,
        optimal_frontrun: 0,
        frontrun_output: 0,
        backrun_output: 0,
        attacker_gross_profit: 0,
        attacker_net_profit: 0,
        victim_loss_absolute: 0,
        victim_extra_slippage_bps: 0,
        attack_feasible: false,
        attack_profitable: false,
        tx_cost_per_leg: cfg.tx_cost_per_leg,
    }
}

fn load_pool_state(row: &LiveSnapshotRow) -> Result<PoolState> {
    let bytes = cached_bytes(row)?;
    <PoolState as CarbonDeserialize>::deserialize(&bytes).context("decode PoolState")
}

fn load_amm_config(row: &LiveSnapshotRow) -> Result<AmmConfig> {
    let bytes = cached_bytes(row)?;
    <AmmConfig as CarbonDeserialize>::deserialize(&bytes).context("decode AmmConfig")
}

fn cached_bytes(row: &LiveSnapshotRow) -> Result<Vec<u8>> {
    let path = row
        .cache_path
        .as_ref()
        .map(PathBuf::from)
        .context("snapshot row has no cache_path")?;
    CachedAccount::read(&path)?.data_bytes()
}

fn validate_tick_arrays(
    candidate: &LiveCandidateReadinessRow,
    snapshots: &AccountSnapshotIndex,
    snapshot_unix: u64,
) -> Result<bool> {
    for account in candidate
        .tick_arrays
        .as_deref()
        .unwrap_or_default()
        .split(';')
        .filter(|account| !account.is_empty())
    {
        let Some(row) = snapshots.pubkey(&candidate.pool_label, snapshot_unix, account) else {
            return Ok(false);
        };
        // This first evaluator uses only the current active-liquidity range.
        // It still requires the referenced remaining accounts to be snapshotted,
        // but full tick-array decoding belongs to the later tick-crossing model.
        let _ = row;
    }
    Ok(true)
}

fn infer_zero_for_one(
    candidate: &LiveCandidateReadinessRow,
    pool_state: &PoolState,
) -> Result<bool> {
    let input_vault = candidate
        .input_vault
        .as_deref()
        .ok_or_else(|| anyhow!("missing input_vault"))?;
    if input_vault == pool_state.token_vault0.to_string() {
        return Ok(true);
    }
    if input_vault == pool_state.token_vault1.to_string() {
        return Ok(false);
    }
    Err(anyhow!("input_vault does not match pool vaults"))
}

fn local_base_input_swap(
    state: LocalClmmState,
    amount_in: u128,
    zero_for_one: bool,
) -> Option<LocalSwapResult> {
    if amount_in == 0 || state.sqrt_price <= 0.0 || state.liquidity <= 0.0 {
        return None;
    }
    let amount_after_fee =
        amount_in as f64 * (FEE_DENOMINATOR - state.trade_fee_rate) / FEE_DENOMINATOR;
    if amount_after_fee <= 0.0 {
        return None;
    }

    if zero_for_one {
        let next_sqrt = 1.0 / (1.0 / state.sqrt_price + amount_after_fee / state.liquidity);
        if !(next_sqrt > 0.0 && next_sqrt < state.sqrt_price) {
            return None;
        }
        let out = state.liquidity * (state.sqrt_price - next_sqrt);
        Some(LocalSwapResult {
            amount_out: out.max(0.0).floor() as u128,
            next_state: LocalClmmState {
                sqrt_price: next_sqrt,
                ..state
            },
        })
    } else {
        let next_sqrt = state.sqrt_price + amount_after_fee / state.liquidity;
        let out = state.liquidity * (1.0 / state.sqrt_price - 1.0 / next_sqrt);
        Some(LocalSwapResult {
            amount_out: out.max(0.0).floor() as u128,
            next_state: LocalClmmState {
                sqrt_price: next_sqrt,
                ..state
            },
        })
    }
}

fn grid_sandwich(
    state: LocalClmmState,
    victim_amount_in: u128,
    victim_min_out: Option<u128>,
    zero_for_one: bool,
    tx_cost_per_leg: u128,
    max_steps: u64,
) -> Option<LocalSandwichResult> {
    if max_steps == 0 {
        return None;
    }
    let fair = local_base_input_swap(state, victim_amount_in, zero_for_one)?;
    let virtual_reserve_in = if zero_for_one {
        state.liquidity / state.sqrt_price
    } else {
        state.liquidity * state.sqrt_price
    };
    let cap = (victim_amount_in.saturating_mul(100))
        .min(virtual_reserve_in.max(1.0).floor() as u128)
        .max(1);

    let mut best: Option<LocalSandwichResult> = None;
    for step in 1..=max_steps {
        let frontrun_amount = ((cap as f64 * step as f64) / max_steps as f64).floor() as u128;
        if frontrun_amount == 0 {
            continue;
        }
        let Some(result) = simulate_sandwich(
            state,
            fair.amount_out,
            victim_amount_in,
            victim_min_out,
            zero_for_one,
            tx_cost_per_leg,
            frontrun_amount,
        ) else {
            continue;
        };
        if best
            .as_ref()
            .map(|best| result.net_profit > best.net_profit)
            .unwrap_or(true)
        {
            best = Some(result);
        }
    }
    best.filter(|result| result.net_profit > 0)
}

fn simulate_sandwich(
    state: LocalClmmState,
    fair_amount_out: u128,
    victim_amount_in: u128,
    victim_min_out: Option<u128>,
    zero_for_one: bool,
    tx_cost_per_leg: u128,
    frontrun_amount: u128,
) -> Option<LocalSandwichResult> {
    let frontrun = local_base_input_swap(state, frontrun_amount, zero_for_one)?;
    let victim = local_base_input_swap(frontrun.next_state, victim_amount_in, zero_for_one)?;
    let backrun = local_base_input_swap(victim.next_state, frontrun.amount_out, !zero_for_one)?;
    let gross_profit = backrun.amount_out as i128 - frontrun_amount as i128;
    let net_profit = gross_profit - (2 * tx_cost_per_leg) as i128;
    let victim_loss_absolute = fair_amount_out.saturating_sub(victim.amount_out);
    let victim_extra_slippage_bps = bps(victim_loss_absolute, fair_amount_out);
    let attack_feasible = victim_min_out
        .map(|min_out| victim.amount_out >= min_out)
        .unwrap_or(false);

    Some(LocalSandwichResult {
        frontrun_amount,
        frontrun_output: frontrun.amount_out,
        backrun_output: backrun.amount_out,
        gross_profit,
        net_profit,
        victim_loss_absolute,
        victim_extra_slippage_bps,
        attack_feasible,
        attack_profitable: net_profit > 0,
    })
}

fn bps(numerator: u128, denominator: u128) -> u64 {
    if denominator == 0 {
        return 0;
    }
    (numerator.saturating_mul(10_000) / denominator).min(u64::MAX as u128) as u64
}

fn bps_diff(a: u128, b: u128) -> u64 {
    let diff = a.abs_diff(b);
    bps(diff, a.max(b))
}
