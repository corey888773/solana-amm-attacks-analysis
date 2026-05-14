use amm_math::multi_fee::{CreatorFeeMode, MultiFeeConfig};
use anchor_lang::prelude::*;

pub const CREATOR_FEE_MODE_DISABLED: u8 = 0;
pub const CREATOR_FEE_MODE_ON_INPUT: u8 = 1;
pub const CREATOR_FEE_MODE_ON_OUTPUT: u8 = 2;

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
    /// Trade fee numerator over `fee_denominator`.
    pub trade_fee_rate: u64,
    /// Creator fee numerator over `fee_denominator`.
    pub creator_fee_rate: u64,
    /// Denominator shared by trade and creator fees.
    pub fee_denominator: u64,
    /// Creator fee mode: 0 disabled, 1 on input, 2 on output.
    pub creator_fee_mode: u8,
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

impl Pool {
    pub fn is_valid_creator_fee_mode(mode: u8) -> bool {
        matches!(
            mode,
            CREATOR_FEE_MODE_DISABLED | CREATOR_FEE_MODE_ON_INPUT | CREATOR_FEE_MODE_ON_OUTPUT
        )
    }

    pub fn fee_config(&self) -> Option<MultiFeeConfig> {
        let creator_fee_mode = match self.creator_fee_mode {
            CREATOR_FEE_MODE_DISABLED => CreatorFeeMode::Disabled,
            CREATOR_FEE_MODE_ON_INPUT => CreatorFeeMode::OnInput,
            CREATOR_FEE_MODE_ON_OUTPUT => CreatorFeeMode::OnOutput,
            _ => return None,
        };

        Some(MultiFeeConfig {
            trade_fee_rate: self.trade_fee_rate,
            creator_fee_rate: self.creator_fee_rate,
            fee_denominator: self.fee_denominator,
            creator_fee_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_with_fee(
        trade_fee_rate: u64,
        creator_fee_rate: u64,
        fee_denominator: u64,
        creator_fee_mode: u8,
    ) -> Pool {
        Pool {
            authority: Pubkey::default(),
            token_a_mint: Pubkey::default(),
            token_b_mint: Pubkey::default(),
            token_a_vault: Pubkey::default(),
            token_b_vault: Pubkey::default(),
            lp_mint: Pubkey::default(),
            trade_fee_rate,
            creator_fee_rate,
            fee_denominator,
            creator_fee_mode,
            reserve_a: 0,
            reserve_b: 0,
            k_last: 0,
            authority_bump: 0,
            pool_bump: 0,
        }
    }

    #[test]
    fn fee_config_supports_single_fee_compatibility() {
        let cfg = pool_with_fee(30, 0, 10_000, CREATOR_FEE_MODE_DISABLED)
            .fee_config()
            .expect("fee config");

        assert_eq!(cfg.trade_fee_rate, 30);
        assert_eq!(cfg.creator_fee_rate, 0);
        assert_eq!(cfg.fee_denominator, 10_000);
        assert!(matches!(cfg.creator_fee_mode, CreatorFeeMode::Disabled));
    }

    #[test]
    fn fee_config_supports_creator_fee_on_input() {
        let cfg = pool_with_fee(2500, 500, 1_000_000, CREATOR_FEE_MODE_ON_INPUT)
            .fee_config()
            .expect("fee config");

        assert_eq!(cfg.trade_fee_rate, 2500);
        assert_eq!(cfg.creator_fee_rate, 500);
        assert_eq!(cfg.fee_denominator, 1_000_000);
        assert!(matches!(cfg.creator_fee_mode, CreatorFeeMode::OnInput));
    }

    #[test]
    fn fee_config_rejects_unknown_creator_fee_mode() {
        assert!(pool_with_fee(30, 0, 10_000, 9).fee_config().is_none());
    }
}
