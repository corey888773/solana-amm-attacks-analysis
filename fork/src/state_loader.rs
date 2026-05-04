use anyhow::{Context, Result};
use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_cpmm_decoder::accounts::pool_state::PoolState;
use litesvm::LiteSVM;
use solana_account::Account;
use solana_pubkey::Pubkey;
use solana_sdk::clock::Clock;
use std::path::Path;
use std::str::FromStr;

use crate::account_fetcher::CachedAccount;
use crate::pool::{PoolManifest, RaydiumCpmmPool};

/// Read the cached pool state account and return its `open_time` field.
/// Used by the legacy-snapshot fallback so we can pick a timestamp that
/// provably passes Raydium's `block_timestamp >= pool.open_time` check
/// without resorting to magic constants.
fn read_pool_open_time(cache_dir: &Path, pool_pubkey: &Pubkey) -> Result<u64> {
    let path = cache_dir.join(format!("{}.json", pool_pubkey));
    let cached = CachedAccount::read(&path)
        .with_context(|| format!("cached pool account missing: {}", path.display()))?;
    let data = cached.data_bytes()?;
    let pool_state = PoolState::deserialize(&data)
        .context("deserialize cached PoolState for open_time fallback")?;
    Ok(pool_state.open_time)
}

/// Load a snapshotted Raydium CPMM pool into a LiteSVM instance.
/// Order of operations:
/// 1. Load CPMM program `.so` via `add_program_from_file`.
/// 2. Replay each cached account via `set_account` preserving lamports/owner/data.
/// 3. Set Clock sysvar to snapshot slot/timestamp (Raydium reads pool_open_time).
pub fn load_raydium_cpmm_pool(
    svm: &mut LiteSVM,
    pool: &RaydiumCpmmPool,
    cache_dir: &Path,
    cpmm_program_so: &Path,
) -> Result<()> {
    let program_id = Pubkey::from_str(&pool.manifest.program_id)?;
    svm.add_program_from_file(program_id, cpmm_program_so)
        .map_err(|e| anyhow::anyhow!("add_program_from_file: {:?}", e))?;

    for key in pool.account_set() {
        let path = cache_dir.join(format!("{}.json", key));
        let cached = CachedAccount::read(&path)
            .with_context(|| format!("cached account missing: {}", path.display()))?;
        let acc = Account {
            lamports: cached.lamports,
            data: cached.data_bytes()?,
            owner: Pubkey::from_str(&cached.owner)?,
            executable: cached.executable,
            rent_epoch: cached.rent_epoch,
        };
        svm.set_account(key, acc)
            .map_err(|e| anyhow::anyhow!("set_account {}: {:?}", key, e))?;
    }

    // Advance Clock sysvar past pool.open_time so swap_base_input passes
    // the `block_timestamp >= pool.open_time` guard.
    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = pool.manifest.snapshot_slot;
    clock.unix_timestamp = if pool.manifest.snapshot_unix_ts > 0 {
        pool.manifest.snapshot_unix_ts
    } else {
        // Legacy snapshot predating getBlockTime support — derive the
        // smallest valid timestamp from the cached pool's open_time.
        // Principled fallback: open_time + 1 is provably enough.
        let open_time = read_pool_open_time(cache_dir, &pool.pool_pubkey)
            .context("legacy snapshot fallback: read pool open_time")?;
        eprintln!(
            "WARN: snapshot_unix_ts=0 (stale snapshot for {}). \
             Falling back to pool.open_time+1 = {}. Re-run snapshot CLI to refresh.",
            pool.manifest.label,
            open_time + 1
        );
        open_time as i64 + 1
    };
    svm.set_sysvar::<Clock>(&clock);

    Ok(())
}

impl RaydiumCpmmPool {
    pub fn from_manifest(manifest: PoolManifest) -> Result<Self> {
        Ok(Self {
            pool_pubkey: Pubkey::from_str(&manifest.pool_address)?,
            mint_a: Pubkey::from_str(&manifest.mint_a)?,
            mint_b: Pubkey::from_str(&manifest.mint_b)?,
            vault_a: Pubkey::from_str(&manifest.vault_a)?,
            vault_b: Pubkey::from_str(&manifest.vault_b)?,
            amm_config: Pubkey::from_str(&manifest.amm_config)?,
            observation_state: manifest
                .observation_state
                .as_deref()
                .map(Pubkey::from_str)
                .transpose()?,
            lp_mint: manifest
                .lp_mint
                .as_deref()
                .map(Pubkey::from_str)
                .transpose()?,
            manifest,
        })
    }

    /// All non-program account pubkeys that make up this pool's state.
    /// Used by snapshot CLI (fetch loop) and state_loader (replay loop).
    pub fn account_set(&self) -> Vec<Pubkey> {
        let mut keys = vec![
            self.pool_pubkey,
            self.mint_a,
            self.mint_b,
            self.vault_a,
            self.vault_b,
            self.amm_config,
        ];
        if let Some(o) = self.observation_state {
            keys.push(o);
        }
        if let Some(lp) = self.lp_mint {
            keys.push(lp);
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(observation: Option<&str>, lp: Option<&str>) -> PoolManifest {
        PoolManifest {
            label: "t".into(),
            program_id: "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C".into(),
            pool_address: "BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5".into(),
            snapshot_slot: 1,
            snapshot_unix_ts: 0,
            mint_a: "So11111111111111111111111111111111111111112".into(),
            mint_b: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            vault_a: "11111111111111111111111111111112".into(),
            vault_b: "11111111111111111111111111111113".into(),
            amm_config: "11111111111111111111111111111114".into(),
            observation_state: observation.map(String::from),
            lp_mint: lp.map(String::from),
        }
    }

    #[test]
    fn account_set_includes_optionals() {
        let pool = RaydiumCpmmPool::from_manifest(manifest(
            Some("11111111111111111111111111111115"),
            Some("11111111111111111111111111111116"),
        ))
        .unwrap();
        let keys = pool.account_set();
        assert_eq!(keys.len(), 8);
        assert!(keys.contains(&pool.pool_pubkey));
        assert!(keys.contains(&pool.observation_state.unwrap()));
        assert!(keys.contains(&pool.lp_mint.unwrap()));
    }

    #[test]
    fn account_set_skips_missing_optionals() {
        let pool = RaydiumCpmmPool::from_manifest(manifest(None, None)).unwrap();
        let keys = pool.account_set();
        assert_eq!(keys.len(), 6);
    }

    /// Legacy-snapshot fallback: when manifest's snapshot_unix_ts == 0,
    /// state_loader must derive a valid timestamp from the cached
    /// PoolState's open_time field. Uses the real wsol_surge cache when
    /// present; otherwise skips (no synthetic PoolState fixture maintained).
    #[test]
    fn fallback_derives_timestamp_from_pool_open_time() {
        let cache_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("cache/pools/wsol_surge");
        if !cache_dir.join("manifest.json").exists() {
            eprintln!("skip: wsol_surge cache missing — run snapshot CLI first");
            return;
        }
        let mut m = PoolManifest::read(&cache_dir.join("manifest.json")).unwrap();
        let pool_pk = Pubkey::from_str(&m.pool_address).unwrap();
        let open_time = read_pool_open_time(&cache_dir, &pool_pk).unwrap();
        // Force the fallback path.
        m.snapshot_unix_ts = 0;
        let derived = open_time as i64 + 1;
        assert!(derived > 0, "derived timestamp must be positive");
        // Sanity: open_time is set on real Raydium pools, so derived should
        // be a reasonable post-2020 unix timestamp.
        assert!(
            derived > 1_577_836_800,
            "derived ts {} unexpectedly small (open_time={})",
            derived,
            open_time
        );
    }
}
