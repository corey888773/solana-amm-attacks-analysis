use anchor_lang::prelude::*;

#[error_code]
pub enum AmmError {
    #[msg("Swap output below minimum (slippage exceeded)")]
    SlippageExceeded,
    #[msg("Invalid fee: must be < 10000 bps")]
    InvalidFee,
    #[msg("Zero amount not allowed")]
    ZeroAmount,
    #[msg("Insufficient liquidity in pool")]
    InsufficientLiquidity,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Invalid mint order: token_a_mint must be lexicographically less than token_b_mint")]
    InvalidMintOrder,
}
