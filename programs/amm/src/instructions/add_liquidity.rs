use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

use crate::constants::*;
use crate::errors::AmmError;
use crate::state::Pool;

pub fn handle_add_liquidity(ctx: Context<AddLiquidity>, amount_a: u64, amount_b: u64) -> Result<()> {
    require!(amount_a > 0 && amount_b > 0, AmmError::ZeroAmount);

    let pool = &mut ctx.accounts.pool;
    let pool_key = pool.key();

    // Calculate LP tokens to mint
    let lp_amount = if pool.reserve_a == 0 && pool.reserve_b == 0 {
        // First deposit: LP = sqrt(amount_a * amount_b) - MIN_INITIAL_LIQUIDITY
        // Source: Uniswap V2 whitepaper (Adams et al., 2020), Section 3.4
        let product = (amount_a as u128) * (amount_b as u128);
        let sqrt = isqrt(product);
        require!(sqrt > MIN_INITIAL_LIQUIDITY as u128, AmmError::InsufficientLiquidity);
        (sqrt - MIN_INITIAL_LIQUIDITY as u128) as u64
    } else {
        // Proportional deposit: LP = min(a/R_a, b/R_b) * total_supply
        let supply = ctx.accounts.lp_mint.supply;
        let lp_a = (amount_a as u128) * (supply as u128) / (pool.reserve_a as u128);
        let lp_b = (amount_b as u128) * (supply as u128) / (pool.reserve_b as u128);
        lp_a.min(lp_b) as u64
    };

    require!(lp_amount > 0, AmmError::InsufficientLiquidity);

    let authority_seeds: &[&[u8]] = &[
        POOL_AUTHORITY_SEED,
        pool_key.as_ref(),
        &[pool.authority_bump],
    ];

    // Transfer token A from user to vault
    transfer_to_vault(
        &ctx.accounts.user_token_a,
        &ctx.accounts.token_a_vault,
        &ctx.accounts.token_a_mint,
        &ctx.accounts.user,
        &ctx.accounts.token_program,
        amount_a,
    )?;

    // Transfer token B from user to vault
    transfer_to_vault(
        &ctx.accounts.user_token_b,
        &ctx.accounts.token_b_vault,
        &ctx.accounts.token_b_mint,
        &ctx.accounts.user,
        &ctx.accounts.token_program,
        amount_b,
    )?;

    // Mint LP tokens to user
    let signer_seeds: &[&[&[u8]]] = &[authority_seeds];
    let mint_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        MintTo {
            mint: ctx.accounts.lp_mint.to_account_info(),
            to: ctx.accounts.user_lp_token.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        },
        signer_seeds,
    );
    token_interface::mint_to(mint_ctx, lp_amount)?;

    // Update pool reserves
    pool.reserve_a += amount_a;
    pool.reserve_b += amount_b;
    pool.k_last = (pool.reserve_a as u128) * (pool.reserve_b as u128);

    Ok(())
}

fn transfer_to_vault<'info>(
    from: &InterfaceAccount<'info, TokenAccount>,
    to: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    authority: &Signer<'info>,
    token_program: &Interface<'info, TokenInterface>,
    amount: u64,
) -> Result<()> {
    let cpi_ctx = CpiContext::new(
        token_program.key(),
        TransferChecked {
            from: from.to_account_info(),
            to: to.to_account_info(),
            mint: mint.to_account_info(),
            authority: authority.to_account_info(),
        },
    );
    token_interface::transfer_checked(cpi_ctx, amount, mint.decimals)?;
    Ok(())
}

/// Integer square root via Newton's method
fn isqrt(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[derive(Accounts)]
pub struct AddLiquidity<'info> {
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

    #[account(
        mut,
        address = pool.lp_mint,
    )]
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
