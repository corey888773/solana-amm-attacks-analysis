use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::*;
use crate::errors::AmmError;
use crate::state::Pool;

pub fn handle_initialize_pool(ctx: Context<InitializePool>, fee_bps: u16) -> Result<()> {
    require!(fee_bps < 10_000, AmmError::InvalidFee);

    let pool = &mut ctx.accounts.pool;
    pool.authority = ctx.accounts.pool_authority.key();
    pool.token_a_mint = ctx.accounts.token_a_mint.key();
    pool.token_b_mint = ctx.accounts.token_b_mint.key();
    pool.token_a_vault = ctx.accounts.token_a_vault.key();
    pool.token_b_vault = ctx.accounts.token_b_vault.key();
    pool.lp_mint = ctx.accounts.lp_mint.key();
    pool.fee_bps = fee_bps;
    pool.reserve_a = 0;
    pool.reserve_b = 0;
    pool.k_last = 0;
    pool.authority_bump = ctx.bumps.pool_authority;
    pool.pool_bump = ctx.bumps.pool;

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
