use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Burn, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::constants::*;
use crate::errors::AmmError;
use crate::state::Pool;

pub fn handle_remove_liquidity(ctx: Context<RemoveLiquidity>, lp_amount: u64) -> Result<()> {
    require!(lp_amount > 0, AmmError::ZeroAmount);

    let pool = &mut ctx.accounts.pool;
    let pool_key = pool.key();
    let supply = ctx.accounts.lp_mint.supply;

    // Proportional withdrawal: amount = reserve * lp_amount / total_supply
    let amount_a = (pool.reserve_a as u128 * lp_amount as u128 / supply as u128) as u64;
    let amount_b = (pool.reserve_b as u128 * lp_amount as u128 / supply as u128) as u64;

    require!(amount_a > 0 && amount_b > 0, AmmError::InsufficientLiquidity);

    let authority_seeds: &[&[u8]] = &[
        POOL_AUTHORITY_SEED,
        pool_key.as_ref(),
        &[pool.authority_bump],
    ];
    let signer_seeds: &[&[&[u8]]] = &[authority_seeds];

    // Burn LP tokens
    let burn_ctx = CpiContext::new(
        ctx.accounts.token_program.key(),
        Burn {
            mint: ctx.accounts.lp_mint.to_account_info(),
            from: ctx.accounts.user_lp_token.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        },
    );
    token_interface::burn(burn_ctx, lp_amount)?;

    // Transfer token A from vault to user
    let transfer_a_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        TransferChecked {
            from: ctx.accounts.token_a_vault.to_account_info(),
            to: ctx.accounts.user_token_a.to_account_info(),
            mint: ctx.accounts.token_a_mint.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        },
        signer_seeds,
    );
    token_interface::transfer_checked(transfer_a_ctx, amount_a, ctx.accounts.token_a_mint.decimals)?;

    // Transfer token B from vault to user
    let transfer_b_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        TransferChecked {
            from: ctx.accounts.token_b_vault.to_account_info(),
            to: ctx.accounts.user_token_b.to_account_info(),
            mint: ctx.accounts.token_b_mint.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        },
        signer_seeds,
    );
    token_interface::transfer_checked(transfer_b_ctx, amount_b, ctx.accounts.token_b_mint.decimals)?;

    // Update pool reserves
    pool.reserve_a -= amount_a;
    pool.reserve_b -= amount_b;
    pool.k_last = (pool.reserve_a as u128) * (pool.reserve_b as u128);

    Ok(())
}

#[derive(Accounts)]
pub struct RemoveLiquidity<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [POOL_SEED, pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump = pool.pool_bump,
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA authority
    #[account(
        seeds = [POOL_AUTHORITY_SEED, pool.key().as_ref()],
        bump = pool.authority_bump,
    )]
    pub pool_authority: UncheckedAccount<'info>,

    #[account(address = pool.token_a_mint)]
    pub token_a_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(address = pool.token_b_mint)]
    pub token_b_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut, address = pool.token_a_vault)]
    pub token_a_vault: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, address = pool.token_b_vault)]
    pub token_b_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, address = pool.lp_mint)]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        token::mint = lp_mint,
        token::authority = user,
    )]
    pub user_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = token_a_mint,
        token::authority = user,
    )]
    pub user_token_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = token_b_mint,
        token::authority = user,
    )]
    pub user_token_b: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}
