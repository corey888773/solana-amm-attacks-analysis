use crate::historical_cpmm::artifacts::{DecodedCandidateRow, SignatureRow, StatusRow};
use crate::historical_cpmm::config::RAYDIUM_FEE_DENOMINATOR;
use crate::historical_cpmm::decode::{full_account_keys, DecodedSwap};
use crate::historical_cpmm::status::AnalysisStatus;
use crate::programs::raydium_cpmm_program_pubkey;
use crate::CachedAccount;
use anyhow::{anyhow, bail, Context, Result};
use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_cpmm_decoder::accounts::{amm_config::AmmConfig, pool_state::PoolState};
use solana_client::rpc_client::RpcClient;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_status_client_types::{
    option_serializer::OptionSerializer, UiTransactionStatusMeta,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct CandidateBuild {
    pub decoded: DecodedCandidateRow,
    pub status: StatusRow,
}

#[derive(Default)]
pub struct StateCache {
    pools: HashMap<String, PoolState>,
    amm_configs: HashMap<String, AmmConfig>,
    snapshot_root: Option<PathBuf>,
}

impl StateCache {
    pub fn new(snapshot_root: impl Into<PathBuf>) -> Self {
        Self {
            pools: HashMap::new(),
            amm_configs: HashMap::new(),
            snapshot_root: Some(snapshot_root.into()),
        }
    }
}

pub fn build_candidate(
    rpc: &RpcClient,
    state_cache: &mut StateCache,
    sig: &SignatureRow,
    tx: &VersionedTransaction,
    meta: &UiTransactionStatusMeta,
    swap: &DecodedSwap,
    tx_cost_per_leg: u128,
) -> Result<CandidateBuild> {
    let keys = full_account_keys(tx, meta);
    let input_vault_index = account_index(&keys, &swap.accounts.input_vault)
        .ok_or_else(|| anyhow!(AnalysisStatus::MissingPreState.as_str()))?;
    let output_vault_index = account_index(&keys, &swap.accounts.output_vault)
        .ok_or_else(|| anyhow!(AnalysisStatus::MissingPreState.as_str()))?;

    let input_pre = token_balance_amount(meta, input_vault_index, true)?
        .ok_or_else(|| anyhow!(AnalysisStatus::MissingTokenBalances.as_str()))?;
    let input_post = token_balance_amount(meta, input_vault_index, false)?
        .ok_or_else(|| anyhow!(AnalysisStatus::MissingTokenBalances.as_str()))?;
    let output_pre = token_balance_amount(meta, output_vault_index, true)?
        .ok_or_else(|| anyhow!(AnalysisStatus::MissingTokenBalances.as_str()))?;
    let output_post = token_balance_amount(meta, output_vault_index, false)?
        .ok_or_else(|| anyhow!(AnalysisStatus::MissingTokenBalances.as_str()))?;

    let actual_input_delta = input_post.saturating_sub(input_pre);
    if actual_input_delta != swap.amount_in {
        bail!(AnalysisStatus::AmountMismatch.as_str());
    }
    let actual_amount_out = output_pre.saturating_sub(output_post);

    let pool_pubkey = Pubkey::from_str(&swap.accounts.pool_state)?;
    let pool = state_cache.pool_state(rpc, &sig.pool_label, &pool_pubkey)?;
    let amm_config_pubkey = Pubkey::from_str(&swap.accounts.amm_config)?;
    let amm_config = state_cache.amm_config(rpc, &sig.pool_label, &amm_config_pubkey)?;

    let direction = direction(&pool, swap)?;
    let creator_fee_mode = creator_fee_mode(
        &pool,
        &swap.accounts.input_mint,
        &swap.accounts.output_mint,
        amm_config.creator_fee_rate,
    );

    let decoded = DecodedCandidateRow {
        pool_type: "raydium_cpmm".to_string(),
        pool_label: sig.pool_label.clone(),
        pool_address: sig.pool_address.clone(),
        slot: sig.slot,
        signature: sig.signature.clone(),
        block_time: sig.block_time,
        instruction_index: swap.instruction_index,
        direction,
        amount_in: swap.amount_in,
        min_amount_out: swap.min_amount_out,
        actual_amount_out: Some(actual_amount_out),
        reserve_in_before: input_pre,
        reserve_out_before: output_pre,
        trade_fee_rate: amm_config.trade_fee_rate,
        creator_fee_rate: amm_config.creator_fee_rate,
        fee_denominator: RAYDIUM_FEE_DENOMINATOR,
        creator_fee_mode,
        tx_cost_per_leg,
    };
    let status = StatusRow {
        pool_type: decoded.pool_type.clone(),
        pool_label: decoded.pool_label.clone(),
        pool_address: decoded.pool_address.clone(),
        slot: decoded.slot,
        signature: decoded.signature.clone(),
        block_time: decoded.block_time,
        instruction_index: Some(decoded.instruction_index),
        swap_variant: Some(swap.swap_variant.clone()),
        direction: Some(decoded.direction.clone()),
        amount_in: Some(decoded.amount_in),
        min_amount_out: decoded.min_amount_out,
        actual_amount_out: decoded.actual_amount_out,
        analysis_status: AnalysisStatus::Included.as_str().to_string(),
        rejection_stage: None,
        rejection_reason: None,
        rejection_detail: None,
    };

    Ok(CandidateBuild { decoded, status })
}

impl StateCache {
    fn pool_state(&mut self, rpc: &RpcClient, label: &str, key: &Pubkey) -> Result<PoolState> {
        let key_string = key.to_string();
        if let Some(pool) = self.pools.get(&key_string) {
            return Ok(pool.clone());
        }
        let pool = if let Some(snapshot_root) = &self.snapshot_root {
            read_cached_pool_state(snapshot_root, label, key)
                .or_else(|_| fetch_pool_state(rpc, key))?
        } else {
            fetch_pool_state(rpc, key)?
        };
        self.pools.insert(key_string, pool.clone());
        Ok(pool)
    }

    fn amm_config(&mut self, rpc: &RpcClient, label: &str, key: &Pubkey) -> Result<AmmConfig> {
        let key_string = key.to_string();
        if let Some(config) = self.amm_configs.get(&key_string) {
            return Ok(config.clone());
        }
        let config = if let Some(snapshot_root) = &self.snapshot_root {
            read_cached_amm_config(snapshot_root, label, key)
                .or_else(|_| fetch_amm_config(rpc, key))?
        } else {
            fetch_amm_config(rpc, key)?
        };
        self.amm_configs.insert(key_string, config.clone());
        Ok(config)
    }
}

fn read_cached_pool_state(snapshot_root: &Path, label: &str, key: &Pubkey) -> Result<PoolState> {
    let path = snapshot_root.join(label).join(format!("{key}.json"));
    let cached = CachedAccount::read(&path)?;
    PoolState::deserialize(&cached.data_bytes()?).context("deserialize cached PoolState")
}

fn read_cached_amm_config(snapshot_root: &Path, label: &str, key: &Pubkey) -> Result<AmmConfig> {
    let path = snapshot_root.join(label).join(format!("{key}.json"));
    let cached = CachedAccount::read(&path)?;
    AmmConfig::deserialize(&cached.data_bytes()?).context("deserialize cached AmmConfig")
}

fn account_index(keys: &[String], pubkey: &str) -> Option<u8> {
    keys.iter()
        .position(|key| key == pubkey)
        .and_then(|idx| u8::try_from(idx).ok())
}

fn token_balance_amount(
    meta: &UiTransactionStatusMeta,
    account_index: u8,
    pre: bool,
) -> Result<Option<u128>> {
    let balances = if pre {
        option_slice(&meta.pre_token_balances)
    } else {
        option_slice(&meta.post_token_balances)
    };
    let Some(balances) = balances else {
        return Ok(None);
    };
    let Some(balance) = balances
        .iter()
        .find(|balance| balance.account_index == account_index)
    else {
        return Ok(None);
    };
    Ok(Some(balance.ui_token_amount.amount.parse::<u128>()?))
}

fn option_slice<T>(value: &OptionSerializer<Vec<T>>) -> Option<&[T]> {
    match value {
        OptionSerializer::Some(items) => Some(items.as_slice()),
        OptionSerializer::None | OptionSerializer::Skip => None,
    }
}

fn fetch_pool_state(rpc: &RpcClient, key: &Pubkey) -> Result<PoolState> {
    let account = rpc
        .get_account(key)
        .with_context(|| format!("fetch pool state {}", key))?;
    if account.owner != raydium_cpmm_program_pubkey() {
        bail!("pool owner mismatch for {}", key);
    }
    PoolState::deserialize(&account.data).context("deserialize PoolState")
}

fn fetch_amm_config(rpc: &RpcClient, key: &Pubkey) -> Result<AmmConfig> {
    let account = rpc
        .get_account(key)
        .with_context(|| format!("fetch amm config {}", key))?;
    if account.owner != raydium_cpmm_program_pubkey() {
        bail!("amm config owner mismatch for {}", key);
    }
    AmmConfig::deserialize(&account.data).context("deserialize AmmConfig")
}

fn direction(pool: &PoolState, swap: &DecodedSwap) -> Result<String> {
    if swap.accounts.input_vault == pool.token_0_vault.to_string()
        && swap.accounts.output_vault == pool.token_1_vault.to_string()
    {
        Ok("token_0_to_token_1".to_string())
    } else if swap.accounts.input_vault == pool.token_1_vault.to_string()
        && swap.accounts.output_vault == pool.token_0_vault.to_string()
    {
        Ok("token_1_to_token_0".to_string())
    } else {
        bail!(AnalysisStatus::VaultMismatch.as_str());
    }
}

fn creator_fee_mode(
    pool: &PoolState,
    input_mint: &str,
    output_mint: &str,
    creator_fee_rate: u64,
) -> String {
    if !pool.enable_creator_fee || creator_fee_rate == 0 {
        return "disabled".to_string();
    }

    let token_0 = pool.token_0_mint.to_string();
    let token_1 = pool.token_1_mint.to_string();
    let input_charged = match pool.creator_fee_on {
        0 => input_mint == token_0 || input_mint == token_1,
        1 => input_mint == token_0,
        2 => input_mint == token_1,
        _ => false,
    };
    let output_charged = match pool.creator_fee_on {
        0 => output_mint == token_0 || output_mint == token_1,
        1 => output_mint == token_0,
        2 => output_mint == token_1,
        _ => false,
    };

    if input_charged {
        "on_input".to_string()
    } else if output_charged {
        "on_output".to_string()
    } else {
        "disabled".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_index_finds_small_indices() {
        let keys = vec!["a".to_string(), "b".to_string()];
        assert_eq!(account_index(&keys, "b"), Some(1));
        assert_eq!(account_index(&keys, "c"), None);
    }
}
