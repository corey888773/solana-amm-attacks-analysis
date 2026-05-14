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

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        trade_fee_rate: u64,
        creator_fee_rate: u64,
        fee_denominator: u64,
        creator_fee_mode: u8,
    ) -> Result<()> {
        instructions::initialize_pool::handle_initialize_pool(
            ctx,
            trade_fee_rate,
            creator_fee_rate,
            fee_denominator,
            creator_fee_mode,
        )
    }

    pub fn add_liquidity(
        ctx: Context<AddLiquidity>,
        amount_a: u64,
        amount_b: u64,
        min_lp_out: u64,
    ) -> Result<()> {
        instructions::add_liquidity::handle_add_liquidity(ctx, amount_a, amount_b, min_lp_out)
    }

    pub fn remove_liquidity(
        ctx: Context<RemoveLiquidity>,
        lp_amount: u64,
        min_a_out: u64,
        min_b_out: u64,
    ) -> Result<()> {
        instructions::remove_liquidity::handle_remove_liquidity(
            ctx, lp_amount, min_a_out, min_b_out,
        )
    }

    pub fn swap(ctx: Context<Swap>, amount_in: u64, min_amount_out: u64) -> Result<()> {
        instructions::swap::handle_swap(ctx, amount_in, min_amount_out)
    }
}
