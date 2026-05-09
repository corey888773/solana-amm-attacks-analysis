use crate::historical_cpmm::status::AnalysisStatus;
use crate::instructions::{SWAP_BASE_INPUT_DISCRIMINATOR, SWAP_BASE_OUTPUT_DISCRIMINATOR};
use crate::programs::RAYDIUM_CPMM_PROGRAM_ID;
use anyhow::{anyhow, Result};
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_status_client_types::{
    option_serializer::OptionSerializer, UiCompiledInstruction, UiInstruction,
    UiPartiallyDecodedInstruction, UiTransactionStatusMeta,
};

const SWAP_BASE_INPUT_LEN: usize = 8 + 8 + 8;

#[derive(Clone, Debug)]
pub struct DecodedSwap {
    pub instruction_index: u32,
    pub swap_variant: String,
    pub amount_in: u128,
    pub min_amount_out: Option<u128>,
    pub accounts: SwapAccounts,
}

#[derive(Clone, Debug)]
pub struct SwapAccounts {
    pub amm_config: String,
    pub pool_state: String,
    pub input_vault: String,
    pub output_vault: String,
    pub input_mint: String,
    pub output_mint: String,
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
    if program_id != RAYDIUM_CPMM_PROGRAM_ID {
        return None;
    }
    let accounts = account_indices
        .iter()
        .filter_map(|account_idx| keys.get(*account_idx as usize).cloned())
        .collect::<Vec<_>>();
    if accounts.get(3).map(String::as_str) != Some(pool_address) {
        return None;
    }
    Some(decode_swap_base_input(instruction_index, data, accounts))
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
            if program_id != RAYDIUM_CPMM_PROGRAM_ID {
                return None;
            }
            if accounts.get(3).map(String::as_str) != Some(pool_address) {
                return None;
            }
            let data = match bs58::decode(data).into_vec() {
                Ok(data) => data,
                Err(err) => return Some(Err(anyhow!("decode inner instruction data: {err}"))),
            };
            Some(decode_swap_base_input(
                instruction_index,
                &data,
                accounts.clone(),
            ))
        }
        _ => None,
    }
}

fn decode_swap_base_input(
    instruction_index: u32,
    data: &[u8],
    accounts: Vec<String>,
) -> Result<DecodedSwap> {
    if data.starts_with(&SWAP_BASE_OUTPUT_DISCRIMINATOR) {
        return Err(anyhow!(AnalysisStatus::UnsupportedSwapInstruction.as_str()));
    }
    if !data.starts_with(&SWAP_BASE_INPUT_DISCRIMINATOR) {
        return Err(anyhow!(AnalysisStatus::UnsupportedSwapInstruction.as_str()));
    }
    if data.len() != SWAP_BASE_INPUT_LEN {
        return Err(anyhow!(
            "swap_base_input length {}, expected {}",
            data.len(),
            SWAP_BASE_INPUT_LEN
        ));
    }
    if accounts.len() < 13 {
        return Err(anyhow!(
            "swap_base_input account count {}, expected >=13",
            accounts.len()
        ));
    }

    let amount_in = u64::from_le_bytes(data[8..16].try_into().unwrap()) as u128;
    let minimum_amount_out = u64::from_le_bytes(data[16..24].try_into().unwrap()) as u128;
    Ok(DecodedSwap {
        instruction_index,
        swap_variant: "swap_base_input".to_string(),
        amount_in,
        min_amount_out: Some(minimum_amount_out),
        accounts: SwapAccounts {
            amm_config: accounts[2].clone(),
            pool_state: accounts[3].clone(),
            input_vault: accounts[6].clone(),
            output_vault: accounts[7].clone(),
            input_mint: accounts[10].clone(),
            output_mint: accounts[11].clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_swap_base_input_payload() {
        let mut data = Vec::from(SWAP_BASE_INPUT_DISCRIMINATOR);
        data.extend_from_slice(&123_u64.to_le_bytes());
        data.extend_from_slice(&99_u64.to_le_bytes());

        let decoded = decode_swap_base_input(
            7,
            &data,
            (0..13).map(|idx| format!("account_{idx}")).collect(),
        )
        .unwrap();
        assert_eq!(decoded.instruction_index, 7);
        assert_eq!(decoded.amount_in, 123);
        assert_eq!(decoded.min_amount_out, Some(99));
        assert_eq!(decoded.accounts.pool_state, "account_3");
        assert_eq!(decoded.accounts.input_vault, "account_6");
    }
}
