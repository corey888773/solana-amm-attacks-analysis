use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

use crate::constants::*;
use crate::errors::AmmError;
use crate::state::Pool;

pub fn handle_add_liquidity(
    ctx: Context<AddLiquidity>,
    amount_a: u64,
    amount_b: u64,
    min_lp_out: u64,
) -> Result<()> {
    require!(amount_a > 0 && amount_b > 0, AmmError::ZeroAmount);

    let pool = &mut ctx.accounts.pool;
    let pool_key = pool.key();

    // Calculate LP tokens to mint.
    // For the first deposit, mint sqrt(a*b) total LP tokens: MIN_INITIAL_LIQUIDITY
    // is locked to the pool authority PDA (dead address) and the rest is given to
    // the user. This prevents the first-depositor inflation attack where a griefer
    // donates reserves to make LP shares unmintably expensive.
    // Source: Uniswap V2 whitepaper (Adams et al., 2020), Section 3.4 — "Initialization
    // of liquidity token supply": the first liquidity provider is credited with
    // sqrt(x*y) - MINIMUM_LIQUIDITY, and MINIMUM_LIQUIDITY is permanently locked.
    let (lp_amount, locked_amount) = if pool.reserve_a == 0 && pool.reserve_b == 0 {
        let product = (amount_a as u128)
            .checked_mul(amount_b as u128)
            .ok_or(AmmError::MathOverflow)?;
        let sqrt = isqrt(product);
        require!(
            sqrt > MIN_INITIAL_LIQUIDITY as u128,
            AmmError::InsufficientLiquidity
        );
        let user_lp = sqrt
            .checked_sub(MIN_INITIAL_LIQUIDITY as u128)
            .ok_or(AmmError::MathOverflow)? as u64;
        (user_lp, MIN_INITIAL_LIQUIDITY)
    } else {
        // Proportional deposit: LP = min(a/R_a, b/R_b) * total_supply
        let supply = ctx.accounts.lp_mint.supply as u128;
        let lp_a = (amount_a as u128)
            .checked_mul(supply)
            .ok_or(AmmError::MathOverflow)?
            / (pool.reserve_a as u128);
        let lp_b = (amount_b as u128)
            .checked_mul(supply)
            .ok_or(AmmError::MathOverflow)?
            / (pool.reserve_b as u128);
        (lp_a.min(lp_b) as u64, 0u64)
    };

    require!(lp_amount > 0, AmmError::InsufficientLiquidity);
    require!(lp_amount >= min_lp_out, AmmError::SlippageExceeded);

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

    let signer_seeds: &[&[&[u8]]] = &[authority_seeds];

    // Mint locked MIN_INITIAL_LIQUIDITY portion to the pool authority PDA (dead
    // address: no private key, no one can sign transfers out). This keeps
    // lp_mint.supply accurately representing total outstanding LP.
    if locked_amount > 0 {
        let lock_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            MintTo {
                mint: ctx.accounts.lp_mint.to_account_info(),
                to: ctx.accounts.pool_authority_lp_token.to_account_info(),
                authority: ctx.accounts.pool_authority.to_account_info(),
            },
            signer_seeds,
        );
        token_interface::mint_to(lock_ctx, locked_amount)?;
    }

    // Mint LP tokens to user
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

    // Update pool reserves using checked arithmetic.
    pool.reserve_a = pool
        .reserve_a
        .checked_add(amount_a)
        .ok_or(AmmError::MathOverflow)?;
    pool.reserve_b = pool
        .reserve_b
        .checked_add(amount_b)
        .ok_or(AmmError::MathOverflow)?;
    pool.k_last = (pool.reserve_a as u128)
        .checked_mul(pool.reserve_b as u128)
        .ok_or(AmmError::MathOverflow)?;

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
        token::token_program = token_program,
    )]
    pub user_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    /// LP token account owned by the pool authority PDA. Acts as the "dead
    /// address" where MIN_INITIAL_LIQUIDITY is permanently locked on first
    /// deposit (see Uniswap V2 whitepaper Section 3.4).
    #[account(
        mut,
        token::mint = lp_mint,
        token::authority = pool_authority,
        token::token_program = token_program,
    )]
    pub pool_authority_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = token_a_mint,
        token::authority = user,
        token::token_program = token_program,
    )]
    pub user_token_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = token_b_mint,
        token::authority = user,
        token::token_program = token_program,
    )]
    pub user_token_b: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}
