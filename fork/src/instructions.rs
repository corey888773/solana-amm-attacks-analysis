// Raydium CPMM instruction builders.
// Discriminator + account layout from carbon-raydium-cpmm-decoder 0.12.0.
//
// Authority PDA seed from raydium-io/raydium-cp-swap: "vault_and_lp_mint_auth_seed".

use solana_pubkey::Pubkey;
use solana_sdk::instruction::{AccountMeta, Instruction};
use std::str::FromStr;

/// Anchor discriminator for `swap_base_input` (sha256("global:swap_base_input")[..8]).
pub const SWAP_BASE_INPUT_DISCRIMINATOR: [u8; 8] = [0x8f, 0xbe, 0x5a, 0xda, 0xc4, 0x1e, 0x33, 0xde];

/// Anchor discriminator for `swap_base_output`; historical MVP detects it but does not evaluate it.
pub const SWAP_BASE_OUTPUT_DISCRIMINATOR: [u8; 8] =
    [0x37, 0xd9, 0x62, 0x56, 0xa3, 0x4a, 0xb4, 0xad];

pub const AUTH_SEED: &[u8] = b"vault_and_lp_mint_auth_seed";

pub fn cpmm_authority(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[AUTH_SEED], program_id)
}

pub fn token_program() -> Pubkey {
    Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
}

/// Build a Raydium CPMM `swap_base_input` instruction.
///
/// Account list (13 accounts) follows the IDL:
/// payer, authority, amm_config, pool_state,
/// input_token_account, output_token_account,
/// input_vault, output_vault,
/// input_token_program, output_token_program,
/// input_token_mint, output_token_mint,
/// observation_state.
#[allow(clippy::too_many_arguments)]
pub fn swap_base_input(
    program_id: &Pubkey,
    payer: &Pubkey,
    amm_config: &Pubkey,
    pool_state: &Pubkey,
    input_token_account: &Pubkey,
    output_token_account: &Pubkey,
    input_vault: &Pubkey,
    output_vault: &Pubkey,
    input_token_program: &Pubkey,
    output_token_program: &Pubkey,
    input_token_mint: &Pubkey,
    output_token_mint: &Pubkey,
    observation_state: &Pubkey,
    amount_in: u64,
    minimum_amount_out: u64,
) -> Instruction {
    let (authority, _bump) = cpmm_authority(program_id);

    let mut data = Vec::with_capacity(8 + 16);
    data.extend_from_slice(&SWAP_BASE_INPUT_DISCRIMINATOR);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&minimum_amount_out.to_le_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(authority, false),
            AccountMeta::new_readonly(*amm_config, false),
            AccountMeta::new(*pool_state, false),
            AccountMeta::new(*input_token_account, false),
            AccountMeta::new(*output_token_account, false),
            AccountMeta::new(*input_vault, false),
            AccountMeta::new(*output_vault, false),
            AccountMeta::new_readonly(*input_token_program, false),
            AccountMeta::new_readonly(*output_token_program, false),
            AccountMeta::new_readonly(*input_token_mint, false),
            AccountMeta::new_readonly(*output_token_mint, false),
            AccountMeta::new(*observation_state, false),
        ],
        data,
    }
}
