use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;
use std::path::Path;

/// Per-pool snapshot manifest. Persisted as `manifest.json` next to the
/// account dumps so a snapshot is self-describing (slot, timestamp, label).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoolManifest {
    pub label: String,
    pub program_id: String,
    pub pool_address: String,
    pub snapshot_slot: u64,
    pub snapshot_unix_ts: i64,
    pub mint_a: String,
    pub mint_b: String,
    pub vault_a: String,
    pub vault_b: String,
    pub amm_config: String,
    pub observation_state: Option<String>,
    pub lp_mint: Option<String>,
}

impl PoolManifest {
    pub fn write(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }
}

/// In-memory handle to a snapshotted Raydium CPMM pool.
/// Built from `PoolManifest` + cached account JSON files.
#[derive(Clone, Debug)]
pub struct RaydiumCpmmPool {
    pub manifest: PoolManifest,
    pub pool_pubkey: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub vault_a: Pubkey,
    pub vault_b: Pubkey,
    pub amm_config: Pubkey,
    pub observation_state: Option<Pubkey>,
    pub lp_mint: Option<Pubkey>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest(with_optionals: bool) -> PoolManifest {
        PoolManifest {
            label: "wsol_surge".into(),
            program_id: "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C".into(),
            pool_address: "BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5".into(),
            snapshot_slot: 318_000_000,
            snapshot_unix_ts: 1_756_900_000,
            mint_a: "So11111111111111111111111111111111111111112".into(),
            mint_b: "3z2tRjNuQjoq6UDcw4zyEPD1Eb5KXMPYb4GWFzVT1DPg".into(),
            vault_a: "11111111111111111111111111111112".into(),
            vault_b: "11111111111111111111111111111113".into(),
            amm_config: "11111111111111111111111111111114".into(),
            observation_state: with_optionals.then(|| "11111111111111111111111111111115".into()),
            lp_mint: with_optionals.then(|| "11111111111111111111111111111116".into()),
        }
    }

    #[test]
    fn pool_manifest_serde_roundtrip() {
        let m = sample_manifest(true);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        m.write(&path).unwrap();
        let m2 = PoolManifest::read(&path).unwrap();
        assert_eq!(m.label, m2.label);
        assert_eq!(m.pool_address, m2.pool_address);
        assert_eq!(m.snapshot_slot, m2.snapshot_slot);
        assert_eq!(m.observation_state, m2.observation_state);
    }

    // RaydiumCpmmPool::from_manifest lives in state_loader; tests for
    // account_set go there to keep Pubkey parsing concerns colocated.
    fn _ensure_sample_compiles() {
        let _ = sample_manifest(false);
    }
}
