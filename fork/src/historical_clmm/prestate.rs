use crate::historical_clmm::artifacts::{DecodedObservationRow, SignatureRow, StatusRow};
use crate::historical_clmm::decode::{full_account_keys, DecodedSwap};
use crate::historical_clmm::status::AnalysisStatus;
use anyhow::{anyhow, bail, Result};
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_status_client_types::{
    option_serializer::OptionSerializer, UiTransactionStatusMeta,
};

#[derive(Clone, Debug)]
pub struct ObservationBuild {
    pub decoded: DecodedObservationRow,
    pub status: StatusRow,
}

pub fn build_observation(
    sig: &SignatureRow,
    tx: &VersionedTransaction,
    meta: &UiTransactionStatusMeta,
    swap: &DecodedSwap,
    tx_cost_per_leg: u128,
) -> Result<ObservationBuild> {
    let keys = full_account_keys(tx, meta);
    let input_vault_index = account_index(&keys, &swap.accounts.input_vault)
        .ok_or_else(|| anyhow!(AnalysisStatus::MissingTokenBalances.as_str()))?;
    let output_vault_index = account_index(&keys, &swap.accounts.output_vault)
        .ok_or_else(|| anyhow!(AnalysisStatus::MissingTokenBalances.as_str()))?;

    let input_pre = token_balance_amount(meta, input_vault_index, true)?;
    let input_post = token_balance_amount(meta, input_vault_index, false)?;
    let output_pre = token_balance_amount(meta, output_vault_index, true)?;
    let output_post = token_balance_amount(meta, output_vault_index, false)?;

    let (amount_in, min_amount_out, actual_amount_out) =
        observed_amounts(swap, input_pre, input_post, output_pre, output_post)?;

    let direction = if swap.is_base_input {
        "base_input".to_string()
    } else {
        "base_output".to_string()
    };
    let historical_state_status =
        "missing_tick_array_pre_state: getTransaction does not include historical account data"
            .to_string();

    let decoded = DecodedObservationRow {
        pool_type: "raydium_clmm".to_string(),
        pool_label: sig.pool_label.clone(),
        pool_address: sig.pool_address.clone(),
        slot: sig.slot,
        signature: sig.signature.clone(),
        block_time: sig.block_time,
        instruction_index: swap.instruction_index,
        swap_variant: swap.swap_variant.clone(),
        direction,
        is_base_input: swap.is_base_input,
        amount_specified: swap.amount,
        other_amount_threshold: swap.other_amount_threshold,
        sqrt_price_limit_x64: swap.sqrt_price_limit_x64,
        amount_in,
        min_amount_out,
        actual_amount_out,
        input_vault_before: input_pre,
        input_vault_after: input_post,
        output_vault_before: output_pre,
        output_vault_after: output_post,
        amm_config: swap.accounts.amm_config.clone(),
        input_vault: swap.accounts.input_vault.clone(),
        output_vault: swap.accounts.output_vault.clone(),
        input_mint: swap.accounts.input_mint.clone(),
        output_mint: swap.accounts.output_mint.clone(),
        observation_state: swap.accounts.observation_state.clone(),
        tick_arrays: swap.accounts.tick_arrays.join(";"),
        historical_state_status,
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
        swap_variant: Some(decoded.swap_variant.clone()),
        direction: Some(decoded.direction.clone()),
        amount_in: decoded.amount_in,
        min_amount_out: decoded.min_amount_out,
        actual_amount_out: decoded.actual_amount_out,
        analysis_status: AnalysisStatus::Included.as_str().to_string(),
        rejection_stage: None,
        rejection_reason: None,
        rejection_detail: None,
    };

    Ok(ObservationBuild { decoded, status })
}

fn observed_amounts(
    swap: &DecodedSwap,
    input_pre: Option<u128>,
    input_post: Option<u128>,
    output_pre: Option<u128>,
    output_post: Option<u128>,
) -> Result<(Option<u128>, Option<u128>, Option<u128>)> {
    let amount_in = match (input_pre, input_post) {
        (Some(pre), Some(post)) => Some(post.saturating_sub(pre)),
        _ => None,
    };
    let actual_amount_out = match (output_pre, output_post) {
        (Some(pre), Some(post)) => Some(pre.saturating_sub(post)),
        _ => None,
    };

    if swap.is_base_input {
        if let Some(observed_in) = amount_in {
            if observed_in != swap.amount {
                bail!(AnalysisStatus::AmountMismatch.as_str());
            }
        }
        Ok((
            amount_in.or(Some(swap.amount)),
            Some(swap.other_amount_threshold),
            actual_amount_out,
        ))
    } else {
        if let Some(observed_out) = actual_amount_out {
            if observed_out != swap.amount {
                bail!(AnalysisStatus::AmountMismatch.as_str());
            }
        }
        Ok((amount_in, None, actual_amount_out.or(Some(swap.amount))))
    }
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
