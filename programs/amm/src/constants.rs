/// Seed for pool authority PDA
pub const POOL_AUTHORITY_SEED: &[u8] = b"pool_authority";
/// Seed for pool state PDA
pub const POOL_SEED: &[u8] = b"pool";
/// Seed for LP mint PDA
pub const LP_MINT_SEED: &[u8] = b"lp_mint";
/// Minimum initial liquidity (burned to prevent donation attacks)
pub const MIN_INITIAL_LIQUIDITY: u64 = 1000;
/// Token decimals for LP mint
pub const LP_DECIMALS: u8 = 6;
