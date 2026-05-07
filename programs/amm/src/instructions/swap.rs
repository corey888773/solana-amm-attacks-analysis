use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::constants::*;
use crate::errors::AmmError;
use crate::state::Pool;

pub fn handle_swap(ctx: Context<Swap>, amount_in: u64, min_amount_out: u64) -> Result<()> {
    require!(amount_in > 0, AmmError::ZeroAmount);

    let pool = &mut ctx.accounts.pool;
    let pool_key = pool.key();

    // Determine swap direction based on which vault user is depositing into
    let (reserve_in, reserve_out) = if ctx.accounts.user_token_in.mint == pool.token_a_mint {
        (pool.reserve_a, pool.reserve_b)
    } else {
        (pool.reserve_b, pool.reserve_a)
    };

    let fee_config = pool.fee_config().ok_or(AmmError::InvalidFee)?;
    // Delegate math to amm-math (shared crate).
    // Source: Uniswap V2 whitepaper (Adams et al., 2020), Section 3.1.1.
    // Multi-fee rounding/model: Raydium CPMM fee math, mirrored in amm-math.
    let result = amm_math::multi_fee::compute_swap_multi_fee(
        reserve_in as u128,
        reserve_out as u128,
        amount_in as u128,
        &fee_config,
    )
    .ok_or(AmmError::MathOverflow)?;

    let amount_out = u64::try_from(result.amount_out).map_err(|_| AmmError::MathOverflow)?;
    let new_reserve_in =
        u64::try_from(result.new_reserve_in).map_err(|_| AmmError::MathOverflow)?;
    let new_reserve_out =
        u64::try_from(result.new_reserve_out).map_err(|_| AmmError::MathOverflow)?;

    require!(amount_out >= min_amount_out, AmmError::SlippageExceeded);

    let authority_seeds: &[&[u8]] = &[
        POOL_AUTHORITY_SEED,
        pool_key.as_ref(),
        &[pool.authority_bump],
    ];
    let signer_seeds: &[&[&[u8]]] = &[authority_seeds];

    // Transfer input tokens from user to vault
    let transfer_in_ctx = CpiContext::new(
        ctx.accounts.token_program.key(),
        TransferChecked {
            from: ctx.accounts.user_token_in.to_account_info(),
            to: ctx.accounts.vault_in.to_account_info(),
            mint: ctx.accounts.mint_in.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        },
    );
    token_interface::transfer_checked(transfer_in_ctx, amount_in, ctx.accounts.mint_in.decimals)?;

    // Transfer output tokens from vault to user
    let transfer_out_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        TransferChecked {
            from: ctx.accounts.vault_out.to_account_info(),
            to: ctx.accounts.user_token_out.to_account_info(),
            mint: ctx.accounts.mint_out.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        },
        signer_seeds,
    );
    token_interface::transfer_checked(
        transfer_out_ctx,
        amount_out,
        ctx.accounts.mint_out.decimals,
    )?;

    // Update pool reserves
    if ctx.accounts.user_token_in.mint == pool.token_a_mint {
        pool.reserve_a = new_reserve_in;
        pool.reserve_b = new_reserve_out;
    } else {
        pool.reserve_b = new_reserve_in;
        pool.reserve_a = new_reserve_out;
    }
    pool.k_last = (pool.reserve_a as u128)
        .checked_mul(pool.reserve_b as u128)
        .ok_or(AmmError::MathOverflow)?;

    Ok(())
}

#[derive(Accounts)]
pub struct Swap<'info> {
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

    #[account(
        constraint = mint_in.key() == pool.token_a_mint || mint_in.key() == pool.token_b_mint,
    )]
    pub mint_in: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        constraint = mint_out.key() == pool.token_a_mint || mint_out.key() == pool.token_b_mint,
        constraint = mint_out.key() != mint_in.key(),
    )]
    pub mint_out: Box<InterfaceAccount<'info, Mint>>,

    /// Vault receiving input tokens (must be one of pool's vaults)
    #[account(
        mut,
        constraint = vault_in.key() == pool.token_a_vault || vault_in.key() == pool.token_b_vault,
    )]
    pub vault_in: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Vault sending output tokens (must be the other vault)
    #[account(
        mut,
        constraint = vault_out.key() == pool.token_a_vault || vault_out.key() == pool.token_b_vault,
        constraint = vault_out.key() != vault_in.key(),
    )]
    pub vault_out: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = mint_in,
        token::authority = user,
        token::token_program = token_program,
        constraint = user_token_in.mint == pool.token_a_mint || user_token_in.mint == pool.token_b_mint,
        constraint = mint_in.key() == user_token_in.mint,
    )]
    pub user_token_in: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = mint_out,
        token::authority = user,
        token::token_program = token_program,
        constraint = user_token_out.mint == pool.token_a_mint || user_token_out.mint == pool.token_b_mint,
        constraint = user_token_out.mint != user_token_in.mint,
        constraint = mint_out.key() == user_token_out.mint,
    )]
    pub user_token_out: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}
