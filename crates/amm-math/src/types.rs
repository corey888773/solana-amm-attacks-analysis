use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// State of a constant-product AMM pool.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PoolState {
    pub reserve_a: u128,
    pub reserve_b: u128,
    pub fee_bps: u16,
}

impl PoolState {
    pub fn new(reserve_a: u128, reserve_b: u128, fee_bps: u16) -> Self {
        Self {
            reserve_a,
            reserve_b,
            fee_bps,
        }
    }

    /// k = reserve_a * reserve_b (constant product invariant)
    pub fn k(&self) -> u128 {
        self.reserve_a * self.reserve_b
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
    pub amount_out: u128,
    /// Compatibility aggregate for callers that only display one fee number.
    ///
    /// For simple-fee CPMM this is the input-token fee. For multi-fee CPMM
    /// this is `trade_fee + creator_fee`; when the creator fee is charged on
    /// output, this total mixes input-token and output-token units, so callers
    /// that need fee attribution should use `trade_fee` / `creator_fee`.
    pub fee_amount: u128,
    /// Trade fee withheld from input token.
    pub trade_fee: u128,
    /// Creator fee withheld according to the selected fee model.
    pub creator_fee: u128,
    /// Price before swap (token_out / token_in)
    pub price_before: f64,
    /// Price after swap
    pub price_after: f64,
    /// Price impact in basis points
    pub price_impact_bps: u64,
    /// Updated reserves
    pub new_reserve_in: u128,
    pub new_reserve_out: u128,
}

/// Result of a full sandwich attack analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichResult {
    /// Optimal frontrun amount (in input token)
    pub frontrun_amount: u128,
    /// Tokens received from frontrun
    pub frontrun_output: u128,
    /// Tokens received from backrun
    pub backrun_output: u128,
    /// Extra slippage imposed on victim (in bps)
    pub victim_extra_slippage_bps: u64,
    /// Profit before tx costs
    pub gross_profit: i128,
    /// Profit after tx costs (2x fees for frontrun + backrun)
    pub net_profit: i128,
    /// Is sandwich profitable?
    pub is_profitable: bool,
}
