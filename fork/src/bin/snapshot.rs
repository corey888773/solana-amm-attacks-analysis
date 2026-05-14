// Snapshot CLI — one-shot fetch of a Raydium CPMM pool's on-chain state.
//
// 1. Fetch pool account, deser PoolState via carbon-raydium-cpmm-decoder.
// 2. Extract dependency closure: amm_config, vaults, mints, observation, lp_mint.
// 3. Fetch each, persist as JSON under fork/cache/pools/<label>/.
// 4. Write manifest.json with all pubkeys + snapshot slot/timestamp.
//
// Usage:
//   cargo run -p fork --bin snapshot -- \
//     --pool BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5 \
//     --label wsol_surge \
//     --rpc https://api.mainnet-beta.solana.com

use anyhow::{bail, Context, Result};
use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_cpmm_decoder::accounts::pool_state::PoolState;
use clap::Parser;
use fork::{
    account_fetcher::AccountFetcher, pool::PoolManifest, programs::RAYDIUM_CPMM_PROGRAM_ID,
};
use solana_pubkey::Pubkey;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(about = "Snapshot a Raydium CPMM pool from Solana mainnet")]
struct Args {
    #[arg(long)]
    pool: String,

    #[arg(long)]
    label: String,

    #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
    rpc: String,

    #[arg(long, default_value = "fork/cache/pools")]
    cache_root: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let cache_dir = args.cache_root.join(&args.label);
    std::fs::create_dir_all(&cache_dir)?;

    let pool_pk = Pubkey::from_str(&args.pool).context("parse pool address")?;
    let fetcher = AccountFetcher::new(&args.rpc, &cache_dir);

    println!("[1/4] Fetching pool state: {}", pool_pk);
    let pool_acc = fetcher.fetch_and_cache(&pool_pk)?;
    let data = pool_acc.data_bytes()?;

    let expected_owner = RAYDIUM_CPMM_PROGRAM_ID;
    if pool_acc.owner != expected_owner {
        bail!(
            "pool owner mismatch — expected CPMM program ({}), got {}",
            expected_owner,
            pool_acc.owner
        );
    }

    let pool_state = PoolState::deserialize(&data)
        .context("deserialize PoolState — likely not a CPMM pool or layout drift")?;
    println!(
        "      slot={} amm_config={} mint0={} mint1={}",
        pool_acc.slot, pool_state.amm_config, pool_state.token_0_mint, pool_state.token_1_mint
    );

    println!("[2/4] Fetching dependency closure...");
    let deps: [(&str, Pubkey); 7] = [
        ("amm_config", pool_state.amm_config),
        ("vault_0", pool_state.token_0_vault),
        ("vault_1", pool_state.token_1_vault),
        ("mint_0", pool_state.token_0_mint),
        ("mint_1", pool_state.token_1_mint),
        ("observation_state", pool_state.observation_key),
        ("lp_mint", pool_state.lp_mint),
    ];
    for (label, key) in &deps {
        let acc = fetcher.fetch_and_cache(key)?;
        println!(
            "      {:<18} {} ({}B, owner={})",
            label,
            key,
            acc.data_bytes()?.len(),
            acc.owner
        );
    }

    // Resolve snapshot_unix_ts via getBlockTime(pool_slot). RPC providers
    // (esp. public mainnet-beta) sometimes return null for block_time on
    // older slots — fall back to wall-clock now() so downstream consumers
    // still get a sane, monotonically-increasing timestamp.
    let snapshot_unix_ts = match fetcher.get_block_time(pool_acc.slot) {
        Ok(ts) => ts,
        Err(e) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            eprintln!(
                "WARN: getBlockTime(slot={}) failed: {:#}. Falling back to system now() = {}",
                pool_acc.slot, e, now
            );
            now
        }
    };
    println!(
        "      block_time={} (slot {})",
        snapshot_unix_ts, pool_acc.slot
    );

    println!("[3/4] Writing manifest...");
    let manifest = PoolManifest {
        label: args.label.clone(),
        program_id: RAYDIUM_CPMM_PROGRAM_ID.to_string(),
        pool_address: pool_pk.to_string(),
        snapshot_slot: pool_acc.slot,
        snapshot_unix_ts,
        mint_a: pool_state.token_0_mint.to_string(),
        mint_b: pool_state.token_1_mint.to_string(),
        vault_a: pool_state.token_0_vault.to_string(),
        vault_b: pool_state.token_1_vault.to_string(),
        amm_config: pool_state.amm_config.to_string(),
        observation_state: Some(pool_state.observation_key.to_string()),
        lp_mint: Some(pool_state.lp_mint.to_string()),
    };
    manifest.write(&cache_dir.join("manifest.json"))?;

    println!(
        "[4/4] Done. Cached {} files in {}",
        deps.len() + 1,
        cache_dir.display()
    );
    Ok(())
}
