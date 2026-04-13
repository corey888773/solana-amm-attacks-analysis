use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct Pool {
    /// PDA authority that owns token vaults
    pub authority: Pubkey,
    /// Mint of token A
    pub token_a_mint: Pubkey,
    /// Mint of token B
    pub token_b_mint: Pubkey,
    /// Vault holding token A reserves
    pub token_a_vault: Pubkey,
    /// Vault holding token B reserves
    pub token_b_vault: Pubkey,
    /// LP token mint
    pub lp_mint: Pubkey,
    /// Fee in basis points (e.g. 30 = 0.30%)
    pub fee_bps: u16,
    /// Cached reserve of token A
    pub reserve_a: u64,
    /// Cached reserve of token B
    pub reserve_b: u64,
    /// k = reserve_a * reserve_b (last recorded)
    pub k_last: u128,
    /// PDA bump for pool authority
    pub authority_bump: u8,
    /// PDA bump for pool account
    pub pool_bump: u8,
}
