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

pub fn default_pools() -> Vec<PoolTarget> {
    vec![
        PoolTarget {
            label: "wsol_surge".to_string(),
            address: "BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5".to_string(),
        },
        PoolTarget {
            label: "wsol_useless".to_string(),
            address: "Q2sPHPdUWFMg7M7wwrQKLrn619cAucfRsmhVJffodSp".to_string(),
        },
        PoolTarget {
            label: "wsol_debt".to_string(),
            address: "9qppy1KXRTFEeWkFaysYHD7eu9GLg5pGXdLkdL51p7EX".to_string(),
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
