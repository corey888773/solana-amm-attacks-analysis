use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// State of a constant-product AMM pool.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PoolState {
    pub reserve_a: u64,
    pub reserve_b: u64,
    pub fee_bps: u16,
}

impl PoolState {
    pub fn new(reserve_a: u64, reserve_b: u64, fee_bps: u16) -> Self {
        Self { reserve_a, reserve_b, fee_bps }
    }

    /// k = reserve_a * reserve_b (constant product invariant)
    pub fn k(&self) -> u128 {
        self.reserve_a as u128 * self.reserve_b as u128
    }

    /// Spot price: how many token_b per 1 token_a (before fees, infinitesimal trade)
    pub fn spot_price(&self) -> f64 {
        self.reserve_b as f64 / self.reserve_a as f64
    }
}

/// Result of a single swap computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapResult {
    /// Tokens received by swapper
    pub amount_out: u64,
    /// Fee collected (in input token)
    pub fee_amount: u64,
    /// Price before swap (token_out / token_in)
    pub price_before: f64,
    /// Price after swap
    pub price_after: f64,
    /// Price impact in basis points
    pub price_impact_bps: u64,
    /// Updated reserves
    pub new_reserve_in: u64,
    pub new_reserve_out: u64,
}

/// Result of a full sandwich attack analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichResult {
    /// Optimal frontrun amount (in input token)
    pub frontrun_amount: u64,
    /// Tokens received from frontrun
    pub frontrun_output: u64,
    /// Tokens received from backrun
    pub backrun_output: u64,
    /// Extra slippage imposed on victim (in bps)
    pub victim_extra_slippage_bps: u64,
    /// Profit before tx costs
    pub gross_profit: i64,
    /// Profit after tx costs (2x fees for frontrun + backrun)
    pub net_profit: i64,
    /// Is sandwich profitable?
    pub is_profitable: bool,
}
