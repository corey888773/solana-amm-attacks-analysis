use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("DEwKoXtDPgme9dVdKVj7eLta7mzFDyGxA9DCssTVEiRz");

#[program]
pub mod amm {
    use super::*;

    pub fn initialize_pool(ctx: Context<InitializePool>, fee_bps: u16) -> Result<()> {
        instructions::initialize_pool::handle_initialize_pool(ctx, fee_bps)
    }

    pub fn add_liquidity(ctx: Context<AddLiquidity>, amount_a: u64, amount_b: u64) -> Result<()> {
        instructions::add_liquidity::handle_add_liquidity(ctx, amount_a, amount_b)
    }

    pub fn remove_liquidity(ctx: Context<RemoveLiquidity>, lp_amount: u64) -> Result<()> {
        instructions::remove_liquidity::handle_remove_liquidity(ctx, lp_amount)
    }

    pub fn swap(ctx: Context<Swap>, amount_in: u64, min_amount_out: u64) -> Result<()> {
        instructions::swap::handle_swap(ctx, amount_in, min_amount_out)
    }
}
