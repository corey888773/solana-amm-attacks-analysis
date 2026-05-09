use crate::historical_clmm::status::AnalysisStatus;
use crate::programs::RAYDIUM_CLMM_PROGRAM_ID;
use anyhow::{anyhow, Result};
use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_clmm_decoder::instructions::{swap::Swap, swap_v2::SwapV2};
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_status_client_types::{
    option_serializer::OptionSerializer, UiCompiledInstruction, UiInstruction,
    UiPartiallyDecodedInstruction, UiTransactionStatusMeta,
};

#[derive(Clone, Debug)]
pub struct DecodedSwap {
    pub instruction_index: u32,
    pub swap_variant: String,
    pub amount: u128,
    pub other_amount_threshold: u128,
    pub sqrt_price_limit_x64: u128,
    pub is_base_input: bool,
    pub accounts: SwapAccounts,
}

#[derive(Clone, Debug)]
pub struct SwapAccounts {
    pub amm_config: String,
    pub pool_state: String,
    pub input_vault: String,
    pub output_vault: String,
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub observation_state: String,
    pub tick_arrays: Vec<String>,
}

pub fn full_account_keys(tx: &VersionedTransaction, meta: &UiTransactionStatusMeta) -> Vec<String> {
    let mut keys: Vec<String> = tx
        .message
        .static_account_keys()
        .iter()
        .map(ToString::to_string)
        .collect();
    if let OptionSerializer::Some(loaded) = &meta.loaded_addresses {
        keys.extend(loaded.writable.iter().cloned());
        keys.extend(loaded.readonly.iter().cloned());
    }
    keys
}

pub fn decode_target_swaps(
    tx: &VersionedTransaction,
    meta: &UiTransactionStatusMeta,
    pool_address: &str,
) -> Vec<Result<DecodedSwap>> {
    let keys = full_account_keys(tx, meta);
    let mut out = Vec::new();
    for (idx, instruction) in tx.message.instructions().iter().enumerate() {
        if let Some(decoded) = decode_compiled(
            idx as u32,
            instruction.program_id_index,
            &instruction.accounts,
            &instruction.data,
            &keys,
            pool_address,
        ) {
            out.push(decoded);
        }
    }

    if let OptionSerializer::Some(inner_groups) = &meta.inner_instructions {
        for group in inner_groups {
            for (inner_idx, instruction) in group.instructions.iter().enumerate() {
                let instruction_index =
                    u32::from(group.index) * 1_000 + u32::try_from(inner_idx).unwrap_or(0);
                if let Some(decoded) =
                    decode_ui_instruction(instruction_index, instruction, &keys, pool_address)
                {
                    out.push(decoded);
                }
            }
        }
    }
    out
}

fn decode_compiled(
    instruction_index: u32,
    program_id_index: u8,
    account_indices: &[u8],
    data: &[u8],
    keys: &[String],
    pool_address: &str,
) -> Option<Result<DecodedSwap>> {
    let program_id = keys.get(program_id_index as usize)?;
    if program_id != RAYDIUM_CLMM_PROGRAM_ID {
        return None;
    }
    let accounts = account_indices
        .iter()
        .filter_map(|account_idx| keys.get(*account_idx as usize).cloned())
        .collect::<Vec<_>>();
    decode_swap(instruction_index, data, accounts, pool_address)
}

fn decode_ui_instruction(
    instruction_index: u32,
    instruction: &UiInstruction,
    keys: &[String],
    pool_address: &str,
) -> Option<Result<DecodedSwap>> {
    match instruction {
        UiInstruction::Compiled(UiCompiledInstruction {
            program_id_index,
            accounts,
            data,
            ..
        }) => {
            let data = match bs58::decode(data).into_vec() {
                Ok(data) => data,
                Err(err) => return Some(Err(anyhow!("decode inner instruction data: {err}"))),
            };
            decode_compiled(
                instruction_index,
                *program_id_index,
                accounts,
                &data,
                keys,
                pool_address,
            )
        }
        UiInstruction::Parsed(
            solana_transaction_status_client_types::UiParsedInstruction::PartiallyDecoded(
                UiPartiallyDecodedInstruction {
                    program_id,
                    accounts,
                    data,
                    ..
                },
            ),
        ) => {
            if program_id != RAYDIUM_CLMM_PROGRAM_ID {
                return None;
            }
            let data = match bs58::decode(data).into_vec() {
                Ok(data) => data,
                Err(err) => return Some(Err(anyhow!("decode inner instruction data: {err}"))),
            };
            decode_swap(instruction_index, &data, accounts.clone(), pool_address)
        }
        _ => None,
    }
}

fn decode_swap(
    instruction_index: u32,
    data: &[u8],
    accounts: Vec<String>,
    pool_address: &str,
) -> Option<Result<DecodedSwap>> {
    if accounts.get(2).map(String::as_str) != Some(pool_address) {
        return None;
    }

    if let Some(swap) = SwapV2::deserialize(data) {
        return Some(decode_swap_v2(instruction_index, swap, accounts));
    }
    if let Some(swap) = Swap::deserialize(data) {
        return Some(decode_swap_v1(instruction_index, swap, accounts));
    }
    Some(Err(anyhow!(
        "{}",
        AnalysisStatus::UnsupportedSwapInstruction.as_str()
    )))
}

fn decode_swap_v1(
    instruction_index: u32,
    swap: Swap,
    accounts: Vec<String>,
) -> Result<DecodedSwap> {
    if accounts.len() < 10 {
        return Err(anyhow!(
            "swap account count {}, expected >=10",
            accounts.len()
        ));
    }
    Ok(DecodedSwap {
        instruction_index,
        swap_variant: "swap".to_string(),
        amount: swap.amount as u128,
        other_amount_threshold: swap.other_amount_threshold as u128,
        sqrt_price_limit_x64: swap.sqrt_price_limit_x64,
        is_base_input: swap.is_base_input,
        accounts: SwapAccounts {
            amm_config: accounts[1].clone(),
            pool_state: accounts[2].clone(),
            input_vault: accounts[5].clone(),
            output_vault: accounts[6].clone(),
            input_mint: None,
            output_mint: None,
            observation_state: accounts[7].clone(),
            tick_arrays: accounts[9..].to_vec(),
        },
    })
}

fn decode_swap_v2(
    instruction_index: u32,
    swap: SwapV2,
    accounts: Vec<String>,
) -> Result<DecodedSwap> {
    if accounts.len() < 13 {
        return Err(anyhow!(
            "swap_v2 account count {}, expected >=13",
            accounts.len()
        ));
    }
    Ok(DecodedSwap {
        instruction_index,
        swap_variant: "swap_v2".to_string(),
        amount: swap.amount as u128,
        other_amount_threshold: swap.other_amount_threshold as u128,
        sqrt_price_limit_x64: swap.sqrt_price_limit_x64,
        is_base_input: swap.is_base_input,
        accounts: SwapAccounts {
            amm_config: accounts[1].clone(),
            pool_state: accounts[2].clone(),
            input_vault: accounts[5].clone(),
            output_vault: accounts[6].clone(),
            input_mint: Some(accounts[11].clone()),
            output_mint: Some(accounts[12].clone()),
            observation_state: accounts[7].clone(),
            tick_arrays: accounts[13..].to_vec(),
        },
    })
}
