use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use std::path::{Path, PathBuf};

/// On-disk representation of a fetched mainnet account.
/// One file per pubkey under `fork/cache/pools/<label>/<pubkey>.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedAccount {
    pub pubkey: String,
    pub slot: u64,
    pub lamports: u64,
    /// Base64-encoded account data — Borsh/Anchor deser happens at load time.
    pub data_b64: String,
    pub owner: String,
    pub executable: bool,
    pub rent_epoch: u64,
}

impl CachedAccount {
    pub fn write(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).context("serialize CachedAccount")?;
        std::fs::write(path, json).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Self> {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&raw).context("deserialize CachedAccount")
    }

    pub fn data_bytes(&self) -> Result<Vec<u8>> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        STANDARD
            .decode(&self.data_b64)
            .context("base64 decode account data")
    }
}

pub struct AccountFetcher {
    rpc: RpcClient,
    cache_dir: PathBuf,
}

impl AccountFetcher {
    pub fn new(rpc_url: impl Into<String>, cache_dir: impl Into<PathBuf>) -> Self {
        let cache_dir = cache_dir.into();
        std::fs::create_dir_all(&cache_dir).ok();
        Self {
            rpc: RpcClient::new_with_commitment(rpc_url.into(), CommitmentConfig::finalized()),
            cache_dir,
        }
    }

    /// Fetch account from RPC and persist to cache. Re-fetches even if cached
    /// (snapshot CLI semantics — explicit refresh).
    pub fn fetch_and_cache(&self, key: &Pubkey) -> Result<CachedAccount> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let resp = self
            .rpc
            .get_account_with_commitment(key, CommitmentConfig::finalized())
            .with_context(|| format!("rpc get_account {}", key))?;
        let slot = resp.context.slot;
        let acc = resp
            .value
            .with_context(|| format!("account {} not found on mainnet", key))?;
        let cached = CachedAccount {
            pubkey: key.to_string(),
            slot,
            lamports: acc.lamports,
            data_b64: STANDARD.encode(&acc.data),
            owner: acc.owner.to_string(),
            executable: acc.executable,
            rent_epoch: acc.rent_epoch,
        };
        let path = self.cache_path(key);
        cached.write(&path)?;
        Ok(cached)
    }

    pub fn read_cached(&self, key: &Pubkey) -> Result<CachedAccount> {
        CachedAccount::read(&self.cache_path(key))
    }

    pub fn cache_path(&self, key: &Pubkey) -> PathBuf {
        self.cache_dir.join(format!("{}.json", key))
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Fetch the unix timestamp for a given slot via `getBlockTime`.
    /// Some RPC providers drop block_time for old slots — caller decides fallback.
    pub fn get_block_time(&self, slot: u64) -> Result<i64> {
        self.rpc
            .get_block_time(slot)
            .with_context(|| format!("rpc get_block_time slot={}", slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_account_serde_roundtrip() {
        let original = CachedAccount {
            pubkey: "11111111111111111111111111111111".to_string(),
            slot: 123_456_789,
            lamports: 2_039_280,
            data_b64: "AAECAwQFBgcICQ==".to_string(),
            owner: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string(),
            executable: false,
            rent_epoch: 361,
        };

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acc.json");
        original.write(&path).unwrap();

        let read_back = CachedAccount::read(&path).unwrap();
        assert_eq!(read_back.pubkey, original.pubkey);
        assert_eq!(read_back.slot, original.slot);
        assert_eq!(read_back.lamports, original.lamports);
        assert_eq!(read_back.data_b64, original.data_b64);
        assert_eq!(read_back.owner, original.owner);
        assert_eq!(read_back.executable, original.executable);
        assert_eq!(read_back.rent_epoch, original.rent_epoch);

        let bytes = read_back.data_bytes().unwrap();
        assert_eq!(bytes, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
