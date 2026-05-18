use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;
use std::str::FromStr;

pub const DEFAULT_RPC_URL: &str = "https://api.mainnet-beta.solana.com";
pub const RAYDIUM_FEE_DENOMINATOR: u64 = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoolTarget {
    pub label: String,
    pub address: String,
}

impl PoolTarget {
    pub fn pubkey(&self) -> Result<Pubkey> {
        Ok(Pubkey::from_str(&self.address)?)
    }
}

/// Default CPMM pools = top-3 by 24h volume on Raydium CPMM at collection
/// time (Raydium API v3 `pools/info/list?poolType=standard`, filtered to
/// `programId == CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C`), with a
/// TVL floor of $200k to exclude wash-trade-suspect pools (vol/TVL > 3).
///
/// Selection captured 2026-05-12 for the 24h empirical run; the legacy
/// contrastive sample (`wsol_surge`, `wsol_debt`) was retired here because
/// neither remained in the top-15 by volume and the elevated-fee probe
/// has no analogue in current top-volume CPMM (the high-fee tier is
/// dominated by sub-$100k-TVL pools).
pub fn default_pools() -> Vec<PoolTarget> {
    vec![
        PoolTarget {
            label: "wsol_ready".to_string(),
            address: "AiP94aqcnsxPfHTQLerdwNACedhmEUxMaaSxevS2Drxm".to_string(),
        },
        PoolTarget {
            label: "wsol_useless".to_string(),
            address: "Q2sPHPdUWFMg7M7wwrQKLrn619cAucfRsmhVJffodSp".to_string(),
        },
        PoolTarget {
            label: "wsol_idle".to_string(),
            address: "AEZjoUACNSpmYHHRNbfknjL8oiBDw6GhtrMm7tZgBfca".to_string(),
        },
    ]
}

pub fn select_pools(labels: &[String]) -> Result<Vec<PoolTarget>> {
    let pools = default_pools();
    if labels.is_empty() {
        return Ok(pools);
    }

    let mut selected = Vec::new();
    for label in labels {
        let Some(pool) = pools.iter().find(|pool| pool.label == *label) else {
            bail!("unknown pool label '{}'", label);
        };
        selected.push(pool.clone());
    }
    Ok(selected)
}
