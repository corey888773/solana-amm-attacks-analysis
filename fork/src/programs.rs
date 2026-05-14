use solana_pubkey::Pubkey;
use std::str::FromStr;

/// Raydium CPMM mainnet program ID.
/// Verified via Raydium V3 API (2026-05-01).
pub const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";

/// Raydium CLMM mainnet program ID.
/// Source: Raydium docs, "CLMM | Build | Developer guides", mainnet address.
pub const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";

pub fn raydium_cpmm_program_pubkey() -> Pubkey {
    Pubkey::from_str(RAYDIUM_CPMM_PROGRAM_ID).expect("hardcoded program id is valid base58")
}

pub fn raydium_clmm_program_pubkey() -> Pubkey {
    Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).expect("hardcoded program id is valid base58")
}
