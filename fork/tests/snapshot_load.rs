// Integration test: load real WSOL/SURGE snapshot from fork/cache/ and verify
// that PoolState deserialization matches the manifest. This proves the
// snapshot → cache → load pipeline is wired correctly.
//
// Test is skipped automatically if the cache hasn't been populated
// (i.e. if `cargo run -p fork --bin snapshot ...` hasn't run yet).

use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_cpmm_decoder::accounts::amm_config::AmmConfig;
use carbon_raydium_cpmm_decoder::accounts::pool_state::PoolState;
use fork::account_fetcher::CachedAccount;
use fork::pool::{PoolManifest, RaydiumCpmmPool};
use std::path::PathBuf;
use std::str::FromStr;

fn snapshot_dir() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).join("cache/pools/wsol_surge")
}

fn skip_if_no_snapshot() -> bool {
    if !snapshot_dir().join("manifest.json").exists() {
        eprintln!(
            "skipping: snapshot not present at {}. Run: cargo run -p fork --bin snapshot -- \
             --pool BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5 --label wsol_surge",
            snapshot_dir().display()
        );
        true
    } else {
        false
    }
}

#[test]
fn manifest_pool_address_matches_known_target() {
    if skip_if_no_snapshot() {
        return;
    }
    let m = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();
    assert_eq!(
        m.pool_address,
        "BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5"
    );
    assert_eq!(m.program_id, "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");
    // WSOL is one of the two mints
    let wsol = "So11111111111111111111111111111111111111112";
    assert!(
        m.mint_a == wsol || m.mint_b == wsol,
        "WSOL must be one of the mints"
    );
}

#[test]
fn pool_state_deserializes_and_matches_manifest() {
    if skip_if_no_snapshot() {
        return;
    }
    let m = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();

    let pool_acc =
        CachedAccount::read(&snapshot_dir().join(format!("{}.json", m.pool_address))).unwrap();
    assert_eq!(pool_acc.owner, m.program_id, "pool owned by CPMM program");

    let data = pool_acc.data_bytes().unwrap();
    let ps = PoolState::deserialize(&data).expect("PoolState deser");

    // Cross-check that the manifest agrees with what's actually inside PoolState.
    assert_eq!(ps.amm_config.to_string(), m.amm_config);
    assert_eq!(ps.token_0_mint.to_string(), m.mint_a);
    assert_eq!(ps.token_1_mint.to_string(), m.mint_b);
    assert_eq!(ps.token_0_vault.to_string(), m.vault_a);
    assert_eq!(ps.token_1_vault.to_string(), m.vault_b);
    assert_eq!(Some(ps.observation_key.to_string()), m.observation_state);
    assert_eq!(Some(ps.lp_mint.to_string()), m.lp_mint);

    // Sanity: status should not be in a "frozen" state for a live pool.
    // (status bit semantics from raydium-cp-swap: 0 = enabled, non-zero = restrictions)
    println!(
        "PoolState OK: status={} mint0_dec={} mint1_dec={} lp_supply={} open_time={}",
        ps.status, ps.mint_0_decimals, ps.mint_1_decimals, ps.lp_supply, ps.open_time
    );
}

#[test]
fn amm_config_deserializes() {
    if skip_if_no_snapshot() {
        return;
    }
    let m = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();
    let cfg_acc =
        CachedAccount::read(&snapshot_dir().join(format!("{}.json", m.amm_config))).unwrap();
    let data = cfg_acc.data_bytes().unwrap();
    let cfg = AmmConfig::deserialize(&data).expect("AmmConfig deser");

    // Trade fee should be in a sane range. CPMM uses 1e6 denominator, so
    // 2500 = 0.25%, 3000 = 0.30%. We expect <= 5% (50000).
    assert!(
        cfg.trade_fee_rate <= 50_000,
        "trade_fee_rate {} out of sane range",
        cfg.trade_fee_rate
    );
    println!(
        "AmmConfig OK: trade_fee={} protocol_fee={} fund_fee={} creator_fee={} create_pool_fee={}",
        cfg.trade_fee_rate,
        cfg.protocol_fee_rate,
        cfg.fund_fee_rate,
        cfg.creator_fee_rate,
        cfg.create_pool_fee
    );
}

#[test]
fn raydium_cpmm_pool_loads_from_cached_manifest() {
    if skip_if_no_snapshot() {
        return;
    }
    let m = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();
    let pool = RaydiumCpmmPool::from_manifest(m).unwrap();

    let keys = pool.account_set();
    assert_eq!(keys.len(), 8, "WSOL/SURGE snapshot should have 8 accounts");

    // every account_set key has a corresponding cache file
    for key in keys {
        let path = snapshot_dir().join(format!("{}.json", key));
        assert!(path.exists(), "missing cache file for {}", key);
    }
}

#[test]
fn cached_accounts_have_real_lamports() {
    // Rent-exempt risk mitigation: verify cloned accounts preserve mainnet
    // lamports — zero or absurdly low values would make LiteSVM reject them.
    if skip_if_no_snapshot() {
        return;
    }
    let m = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();
    let pool = RaydiumCpmmPool::from_manifest(m).unwrap();
    for key in pool.account_set() {
        let acc = CachedAccount::read(&snapshot_dir().join(format!("{}.json", key))).unwrap();
        assert!(
            acc.lamports >= 890_880,
            "account {} has lamports={} — below typical rent-exempt floor",
            key,
            acc.lamports
        );
        // Pubkey parses
        solana_pubkey::Pubkey::from_str(&acc.pubkey).expect("pubkey parse");
    }
}
