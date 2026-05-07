use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::*;
use crate::errors::AmmError;
use crate::state::{Pool, CREATOR_FEE_MODE_DISABLED};

pub fn handle_initialize_pool(
    ctx: Context<InitializePool>,
    trade_fee_rate: u64,
    creator_fee_rate: u64,
    fee_denominator: u64,
    creator_fee_mode: u8,
) -> Result<()> {
    validate_fee_config(
        trade_fee_rate,
        creator_fee_rate,
        fee_denominator,
        creator_fee_mode,
    )?;

    let pool = &mut ctx.accounts.pool;
    pool.authority = ctx.accounts.pool_authority.key();
    pool.token_a_mint = ctx.accounts.token_a_mint.key();
    pool.token_b_mint = ctx.accounts.token_b_mint.key();
    pool.token_a_vault = ctx.accounts.token_a_vault.key();
    pool.token_b_vault = ctx.accounts.token_b_vault.key();
    pool.lp_mint = ctx.accounts.lp_mint.key();
    pool.trade_fee_rate = trade_fee_rate;
    pool.creator_fee_rate = creator_fee_rate;
    pool.fee_denominator = fee_denominator;
    pool.creator_fee_mode = creator_fee_mode;
    pool.reserve_a = 0;
    pool.reserve_b = 0;
    pool.k_last = 0;
    pool.authority_bump = ctx.bumps.pool_authority;
    pool.pool_bump = ctx.bumps.pool;

    Ok(())
}

pub(crate) fn validate_fee_config(
    trade_fee_rate: u64,
    creator_fee_rate: u64,
    fee_denominator: u64,
    creator_fee_mode: u8,
) -> Result<()> {
    require!(fee_denominator > 0, AmmError::InvalidFee);
    require!(trade_fee_rate < fee_denominator, AmmError::InvalidFee);
    require!(
        trade_fee_rate
            .checked_add(creator_fee_rate)
            .is_some_and(|fee| fee < fee_denominator),
        AmmError::InvalidFee
    );
    require!(
        Pool::is_valid_creator_fee_mode(creator_fee_mode),
        AmmError::InvalidFee
    );
    require!(
        creator_fee_rate == 0 || creator_fee_mode != CREATOR_FEE_MODE_DISABLED,
        AmmError::InvalidFee
    );

    Ok(())
}

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + Pool::INIT_SPACE,
        seeds = [POOL_SEED, token_a_mint.key().as_ref(), token_b_mint.key().as_ref()],
        bump,
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA authority for vaults, no data
    #[account(
        seeds = [POOL_AUTHORITY_SEED, pool.key().as_ref()],
        bump,
    )]
    pub pool_authority: UncheckedAccount<'info>,

    #[account(
        constraint = token_a_mint.key() < token_b_mint.key() @ AmmError::InvalidMintOrder,
    )]
    pub token_a_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_b_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = payer,
        token::mint = token_a_mint,
        token::authority = pool_authority,
        token::token_program = token_program,
        seeds = [VAULT_A_SEED, pool.key().as_ref()],
        bump,
    )]
    pub token_a_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = payer,
        token::mint = token_b_mint,
        token::authority = pool_authority,
        token::token_program = token_program,
        seeds = [VAULT_B_SEED, pool.key().as_ref()],
        bump,
    )]
    pub token_b_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = payer,
        mint::decimals = LP_DECIMALS,
        mint::authority = pool_authority,
        mint::token_program = token_program,
        seeds = [LP_MINT_SEED, pool.key().as_ref()],
        bump,
    )]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CREATOR_FEE_MODE_ON_INPUT, CREATOR_FEE_MODE_ON_OUTPUT};

    #[test]
    fn accepts_single_fee_config() {
        assert!(validate_fee_config(30, 0, 10_000, CREATOR_FEE_MODE_DISABLED).is_ok());
    }

    #[test]
    fn accepts_creator_fee_config() {
        assert!(validate_fee_config(2500, 500, 1_000_000, CREATOR_FEE_MODE_ON_INPUT).is_ok());
        assert!(validate_fee_config(2500, 500, 1_000_000, CREATOR_FEE_MODE_ON_OUTPUT).is_ok());
    }

    #[test]
    fn rejects_invalid_fee_config() {
        assert!(validate_fee_config(30, 0, 0, CREATOR_FEE_MODE_DISABLED).is_err());
        assert!(validate_fee_config(10_000, 0, 10_000, CREATOR_FEE_MODE_DISABLED).is_err());
        assert!(validate_fee_config(9_000, 1_000, 10_000, CREATOR_FEE_MODE_ON_INPUT).is_err());
        assert!(validate_fee_config(30, 1, 10_000, CREATOR_FEE_MODE_DISABLED).is_err());
        assert!(validate_fee_config(30, 0, 10_000, 9).is_err());
    }
}
