use crate::historical_clmm::artifacts::{DecodedObservationRow, StateProbeRow};
use crate::programs::raydium_clmm_program_pubkey;
use anyhow::{Context, Result};
use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_clmm_decoder::accounts::{
    amm_config::AmmConfig, pool_state::PoolState, tick_array_state::TickArrayState,
};
use solana_account::Account;
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Clone, Debug)]
struct CurrentAccount {
    slot: u64,
    account: Account,
}

#[derive(Default)]
struct CurrentAccountCache {
    accounts: HashMap<String, Option<CurrentAccount>>,
}

pub fn probe_state_requirements(
    rpc: &RpcClient,
    decoded: &[DecodedObservationRow],
) -> Vec<StateProbeRow> {
    let mut cache = CurrentAccountCache::default();
    decoded
        .iter()
        .map(|row| probe_row(rpc, &mut cache, row))
        .collect()
}

fn probe_row(
    rpc: &RpcClient,
    cache: &mut CurrentAccountCache,
    row: &DecodedObservationRow,
) -> StateProbeRow {
    let tick_arrays = split_tick_arrays(&row.tick_arrays);
    let pool = cache.get(rpc, &row.pool_address).ok().flatten();
    let amm_config = cache.get(rpc, &row.amm_config).ok().flatten();
    let observation = cache.get(rpc, &row.observation_state).ok().flatten();
    let tick_results = tick_arrays
        .iter()
        .filter_map(|key| cache.get(rpc, key).ok().flatten())
        .collect::<Vec<_>>();

    let current_pool_state_ok = pool.as_ref().is_some_and(|account| {
        is_clmm_owned(account) && PoolState::deserialize(&account.account.data).is_some()
    });
    let current_amm_config_ok = amm_config.as_ref().is_some_and(|account| {
        is_clmm_owned(account) && AmmConfig::deserialize(&account.account.data).is_some()
    });
    let current_observation_state_found = observation.as_ref().is_some_and(is_clmm_owned);
    let current_remaining_accounts_found = tick_results.len();
    let current_remaining_accounts_missing = tick_arrays.len().saturating_sub(tick_results.len());
    let current_tick_arrays_ok = tick_results
        .iter()
        .filter(|account| {
            is_clmm_owned(account) && TickArrayState::deserialize(&account.account.data).is_some()
        })
        .count();
    let current_tick_array_decode_failures =
        current_remaining_accounts_found.saturating_sub(current_tick_arrays_ok);
    let current_probe_slot = [
        pool.as_ref(),
        amm_config.as_ref(),
        observation.as_ref(),
        tick_results.first(),
    ]
    .into_iter()
    .flatten()
    .map(|account| account.slot)
    .min();

    StateProbeRow {
        pool_type: row.pool_type.clone(),
        pool_label: row.pool_label.clone(),
        pool_address: row.pool_address.clone(),
        slot: row.slot,
        signature: row.signature.clone(),
        instruction_index: row.instruction_index,
        pool_state: row.pool_address.clone(),
        amm_config: row.amm_config.clone(),
        observation_state: row.observation_state.clone(),
        tick_arrays: row.tick_arrays.clone(),
        required_account_count: 3 + tick_arrays.len(),
        current_probe_slot,
        current_pool_state_ok,
        current_amm_config_ok,
        current_observation_state_found,
        current_remaining_accounts_found,
        current_remaining_accounts_missing,
        current_tick_arrays_ok,
        current_tick_array_decode_failures,
        historical_state_available: false,
        candidate_ready: false,
        blocker: "need historical PoolState/AmmConfig/TickArrayState at victim pre-state slot; current RPC probe is schema sanity only".to_string(),
    }
}

impl CurrentAccountCache {
    fn get(&mut self, rpc: &RpcClient, key: &str) -> Result<Option<CurrentAccount>> {
        if let Some(account) = self.accounts.get(key) {
            return Ok(account.clone());
        }
        let pubkey = Pubkey::from_str(key).with_context(|| format!("parse pubkey {key}"))?;
        let response = rpc
            .get_account_with_commitment(&pubkey, CommitmentConfig::finalized())
            .with_context(|| format!("get account {key}"))?;
        let account = response.value.map(|account| CurrentAccount {
            slot: response.context.slot,
            account,
        });
        self.accounts.insert(key.to_string(), account.clone());
        Ok(account)
    }
}

fn split_tick_arrays(value: &str) -> Vec<String> {
    value
        .split(';')
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn is_clmm_owned(account: &CurrentAccount) -> bool {
    account.account.owner == raydium_clmm_program_pubkey()
}
