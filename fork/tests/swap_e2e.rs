// Faza 4 — single swap e2e on cloned WSOL/SURGE pool.
// Validates: cloned pool accepts a real swap_base_input ix, reserves move
// in the expected direction, output token account credited.

use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_cpmm_decoder::accounts::amm_config::AmmConfig;
use fork::cheat::{
    fund_token_account, read_token_amount, SPL_TOKEN_PROGRAM, TOKEN_RENT_EXEMPT_LAMPORTS,
};
use fork::instructions::{cpmm_authority, swap_base_input};
use fork::pool::{PoolManifest, RaydiumCpmmPool};
use fork::programs::raydium_cpmm_program_pubkey;
use fork::state_loader::load_raydium_cpmm_pool;
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::path::PathBuf;
use std::str::FromStr;

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
    !snapshot_dir().join("manifest.json").exists() || !cpmm_so().exists()
}

#[test]
fn swap_wsol_for_surge_on_cloned_pool() {
    if skip_if_missing() {
        eprintln!("skipping: snapshot or .so missing");
        return;
    }

    // 1. Setup LiteSVM with cloned mainnet pool.
    let manifest = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();
    let pool = RaydiumCpmmPool::from_manifest(manifest).unwrap();

    let mut svm = LiteSVM::default()
        .with_builtins()
        .with_default_programs()
        .with_sysvars()
        .with_lamports(1_000_000_000_000)
        .with_sigverify(false)
        .with_blockhash_check(false);

    load_raydium_cpmm_pool(&mut svm, &pool, &snapshot_dir(), &cpmm_so()).unwrap();

    // 2. Confirm trade fee config from snapshot
    let cfg_acc = svm.get_account(&pool.amm_config).unwrap();
    let cfg = AmmConfig::deserialize(&cfg_acc.data).expect("AmmConfig deser");
    assert_eq!(cfg.trade_fee_rate, 2500, "expected 0.25% trade fee");

    // 3. Setup attacker keypair, airdrop SOL for tx fees + token account rent
    let attacker = Keypair::new();
    svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();

    // 4. Cheat-create attacker WSOL + SURGE token accounts.
    // For WSOL we fund 1 SOL (1e9 lamports of WSOL).
    let attacker_wsol = Keypair::new().pubkey();
    let attacker_surge = Keypair::new().pubkey();

    let amount_in: u64 = 1_000_000_000; // 1 WSOL
    fund_token_account(
        &mut svm,
        &attacker_wsol,
        &pool.mint_a,
        &attacker.pubkey(),
        amount_in,
    )
    .unwrap();
    fund_token_account(
        &mut svm,
        &attacker_surge,
        &pool.mint_b,
        &attacker.pubkey(),
        0,
    )
    .unwrap();

    // 5. Capture pre-swap state.
    let vault_a_pre = read_token_amount(&svm, &pool.vault_a);
    let vault_b_pre = read_token_amount(&svm, &pool.vault_b);
    let attacker_wsol_pre = read_token_amount(&svm, &attacker_wsol);
    let attacker_surge_pre = read_token_amount(&svm, &attacker_surge);
    println!(
        "PRE  vault_a={} vault_b={} att_wsol={} att_surge={}",
        vault_a_pre, vault_b_pre, attacker_wsol_pre, attacker_surge_pre
    );

    // 6. Build swap_base_input ix: WSOL (mint_a) → SURGE (mint_b)
    let token_program = Pubkey::from_str(SPL_TOKEN_PROGRAM).unwrap();
    let program_id = raydium_cpmm_program_pubkey();
    let ix = swap_base_input(
        &program_id,
        &attacker.pubkey(),
        &pool.amm_config,
        &pool.pool_pubkey,
        &attacker_wsol,  // input_token_account
        &attacker_surge, // output_token_account
        &pool.vault_a,   // input_vault
        &pool.vault_b,   // output_vault
        &token_program,
        &token_program,
        &pool.mint_a,
        &pool.mint_b,
        &pool.observation_state.unwrap(),
        amount_in,
        0, // minimum_amount_out — accept any output for smoke test
    );

    // 7. Send.
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&attacker.pubkey()), &blockhash);
    let tx = Transaction::new(&[&attacker], msg, blockhash);
    let result = svm.send_transaction(tx);
    match result {
        Ok(meta) => {
            println!(
                "swap OK: cu={} logs={}",
                meta.compute_units_consumed,
                meta.logs.len()
            );
        }
        Err(e) => {
            for l in &e.meta.logs {
                eprintln!("  log: {}", l);
            }
            panic!("swap failed: {:?}", e.err);
        }
    }

    // 8. Verify post-state.
    let vault_a_post = read_token_amount(&svm, &pool.vault_a);
    let vault_b_post = read_token_amount(&svm, &pool.vault_b);
    let attacker_wsol_post = read_token_amount(&svm, &attacker_wsol);
    let attacker_surge_post = read_token_amount(&svm, &attacker_surge);
    println!(
        "POST vault_a={} vault_b={} att_wsol={} att_surge={}",
        vault_a_post, vault_b_post, attacker_wsol_post, attacker_surge_post
    );

    assert_eq!(attacker_wsol_post, 0, "all WSOL spent");
    assert!(attacker_surge_post > 0, "SURGE received");
    assert_eq!(
        vault_a_post,
        vault_a_pre + amount_in,
        "input_vault += amount_in"
    );
    assert!(vault_b_post < vault_b_pre, "output_vault decreased");
    let vault_out_delta = vault_b_pre - vault_b_post;
    assert_eq!(
        attacker_surge_post, vault_out_delta,
        "attacker SURGE received == output_vault delta"
    );

    // 9. Compare to off-chain constant-product math (with trade fee 0.25%).
    // Formula: out = (reserve_out * input_after_fee) / (reserve_in + input_after_fee)
    let fee_num: u128 = cfg.trade_fee_rate as u128;
    let fee_denom: u128 = 1_000_000;
    let amount_in_u: u128 = amount_in as u128;
    let trade_fee = amount_in_u * fee_num / fee_denom
        + if (amount_in_u * fee_num) % fee_denom != 0 {
            1
        } else {
            0
        };
    let input_after_fee = amount_in_u - trade_fee;
    let reserve_in = vault_a_pre as u128;
    let reserve_out = vault_b_pre as u128;
    let expected_out = (reserve_out * input_after_fee) / (reserve_in + input_after_fee);
    println!(
        "off-chain expected_out={} | on-chain attacker_surge_post={} | diff={}",
        expected_out,
        attacker_surge_post,
        (attacker_surge_post as i128) - (expected_out as i128)
    );
    let diff = (attacker_surge_post as i128 - expected_out as i128).abs();
    let rel_err = diff as f64 / expected_out as f64;
    println!(
        "relative error vs naive (trade_fee only): {:.6}%",
        rel_err * 100.0
    );

    // Naive off-chain (trade_fee=0.25% only) overestimates by ~0.06%, consistent
    // with CPMM also applying creator_fee on input (cfg.creator_fee_rate=500=0.05%)
    // when pool_state.enable_creator_fee && creator_fee_on==Input.
    // TODO: extend amm-math::compute_swap with multi-fee variant matching
    // pool_state.{enable_creator_fee, creator_fee_on}.
    assert!(
        rel_err < 0.001,
        "naive off-chain should match on-chain within 0.1% (got {:.4}%)",
        rel_err * 100.0
    );

    let _ = TOKEN_RENT_EXEMPT_LAMPORTS;
    let _ = cpmm_authority;
}
