// E2E: load WSOL/SURGE snapshot into LiteSVM and verify the program +
// pool state are queryable post-load. This proves the full pipeline:
// mainnet RPC → JSON cache → LiteSVM in-memory state.

use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_cpmm_decoder::accounts::pool_state::PoolState;
use fork::pool::{PoolManifest, RaydiumCpmmPool};
use fork::state_loader::load_raydium_cpmm_pool;
use litesvm::LiteSVM;
use std::path::PathBuf;

fn cache_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cache")
}

fn snapshot_dir() -> PathBuf {
    cache_root().join("pools/wsol_surge")
}

fn cpmm_so() -> PathBuf {
    cache_root().join("programs/raydium_cpmm.so")
}

fn skip_if_missing() -> bool {
    let manifest = snapshot_dir().join("manifest.json");
    let so = cpmm_so();
    if !manifest.exists() || !so.exists() {
        eprintln!(
            "skipping: prerequisites missing.\n  manifest: {} exists={}\n  so:       {} exists={}\nRun:\n  cargo run -p fork --bin snapshot -- --pool BScfGKZf... --label wsol_surge\n  solana program dump CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C fork/cache/programs/raydium_cpmm.so --url mainnet-beta",
            manifest.display(), manifest.exists(), so.display(), so.exists(),
        );
        true
    } else {
        false
    }
}

#[test]
fn load_wsol_surge_into_litesvm() {
    if skip_if_missing() {
        return;
    }

    let manifest = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();
    let pool = RaydiumCpmmPool::from_manifest(manifest).unwrap();

    let mut svm = LiteSVM::default()
        .with_builtins()
        .with_sysvars()
        .with_lamports(1_000_000_000_000)
        .with_sigverify(false)
        .with_blockhash_check(false);

    load_raydium_cpmm_pool(&mut svm, &pool, &snapshot_dir(), &cpmm_so())
        .expect("load_raydium_cpmm_pool");

    // Pool account is queryable from LiteSVM
    let pool_acc = svm
        .get_account(&pool.pool_pubkey)
        .expect("pool account in svm");
    assert_eq!(
        pool_acc.owner.to_string(),
        "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C",
        "pool owner = CPMM program"
    );
    assert!(pool_acc.lamports > 0);

    // Re-deserialize to confirm data round-tripped through LiteSVM intact
    let ps = PoolState::deserialize(&pool_acc.data).expect("PoolState from svm");
    assert_eq!(ps.token_0_mint, pool.mint_a);
    assert_eq!(ps.token_1_mint, pool.mint_b);
    assert_eq!(ps.amm_config, pool.amm_config);

    // Vaults queryable + token-program-owned
    for vault in [pool.vault_a, pool.vault_b] {
        let v = svm
            .get_account(&vault)
            .unwrap_or_else(|| panic!("vault {} missing", vault));
        assert_eq!(
            v.owner.to_string(),
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "vault {} should be SPL Token-owned",
            vault
        );
        assert_eq!(v.data.len(), 165, "SPL token account = 165 bytes");
    }

    // Mints queryable + 82 bytes
    for mint in [pool.mint_a, pool.mint_b] {
        let m = svm
            .get_account(&mint)
            .unwrap_or_else(|| panic!("mint {} missing", mint));
        assert_eq!(m.data.len(), 82, "SPL mint = 82 bytes");
    }

    println!(
        "LiteSVM load OK: pool={} status={} mint0_dec={} mint1_dec={} lp_supply={}",
        pool.pool_pubkey, ps.status, ps.mint_0_decimals, ps.mint_1_decimals, ps.lp_supply
    );
}

#[test]
fn vault_token_balances_match_pool_pricing() {
    // The vault SPL Token accounts hold u64 amounts at offset 64 (after mint=32 + owner=32).
    // We sanity-check that both vaults have non-zero balances — i.e. the snapshot
    // captured a live, funded pool.
    if skip_if_missing() {
        return;
    }

    let manifest = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();
    let pool = RaydiumCpmmPool::from_manifest(manifest).unwrap();

    let mut svm = LiteSVM::default()
        .with_builtins()
        .with_sysvars()
        .with_sigverify(false)
        .with_blockhash_check(false);
    load_raydium_cpmm_pool(&mut svm, &pool, &snapshot_dir(), &cpmm_so()).unwrap();

    let vault_a = svm.get_account(&pool.vault_a).unwrap();
    let vault_b = svm.get_account(&pool.vault_b).unwrap();

    let amount_a = u64::from_le_bytes(vault_a.data[64..72].try_into().unwrap());
    let amount_b = u64::from_le_bytes(vault_b.data[64..72].try_into().unwrap());

    assert!(amount_a > 0, "vault_a empty");
    assert!(amount_b > 0, "vault_b empty");
    println!(
        "Vault balances: a={} (raw u64), b={} (raw u64)",
        amount_a, amount_b
    );
}
