use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;
use std::str::FromStr;

pub const DEFAULT_RPC_URL: &str = "https://api.mainnet-beta.solana.com";

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
            label: "clmm_wsol_usdc".to_string(),
            address: "3ucNos4NbumPLZNWztqGHNFFgkHeRMBQAVemeeomsUxv".to_string(),
        },
        PoolTarget {
            label: "clmm_wsol_usdt".to_string(),
            address: "3nMFwZXwY1s1M5s8vYAHqd4wGs4iSxXE4LRoUMMYqEgF".to_string(),
        },
        PoolTarget {
            label: "clmm_usdc_usdt".to_string(),
            address: "BZtgQEyS6eXUXicYPHecYQ7PybqodXQMvkjUbP4R8mUU".to_string(),
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
            bail!("unknown CLMM pool label '{}'", label);
        };
        selected.push(pool.clone());
    }
    Ok(selected)
}
