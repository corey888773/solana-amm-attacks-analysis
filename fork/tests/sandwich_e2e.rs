// Faza 5 — full 3-tx sandwich on cloned WSOL/SURGE pool.
// frontrun (attacker WSOL→SURGE) → victim (WSOL→SURGE) → backrun (attacker SURGE→WSOL)
// Asserts: backrun output > frontrun input ⇒ sandwich profitable.

use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_cpmm_decoder::accounts::amm_config::AmmConfig;
use fork::cheat::{fund_token_account, read_token_amount, SPL_TOKEN_PROGRAM};
use fork::instructions::swap_base_input;
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
fn snapshot_dir() -> PathBuf { cache_root().join("pools/wsol_surge") }
fn cpmm_so() -> PathBuf { cache_root().join("programs/raydium_cpmm.so") }

fn skip_if_missing() -> bool {
    !snapshot_dir().join("manifest.json").exists() || !cpmm_so().exists()
}

#[allow(clippy::too_many_arguments)]
fn build_swap(
    pool: &RaydiumCpmmPool,
    program_id: Pubkey,
    payer: &Pubkey,
    input_account: &Pubkey,
    output_account: &Pubkey,
    a_to_b: bool,
    amount_in: u64,
) -> solana_sdk::instruction::Instruction {
    let token_program = Pubkey::from_str(SPL_TOKEN_PROGRAM).unwrap();
    let (input_vault, output_vault, input_mint, output_mint) = if a_to_b {
        (pool.vault_a, pool.vault_b, pool.mint_a, pool.mint_b)
    } else {
        (pool.vault_b, pool.vault_a, pool.mint_b, pool.mint_a)
    };
    swap_base_input(
        &program_id, payer, &pool.amm_config, &pool.pool_pubkey,
        input_account, output_account,
        &input_vault, &output_vault,
        &token_program, &token_program,
        &input_mint, &output_mint,
        &pool.observation_state.unwrap(),
        amount_in, 0,
    )
}

fn send_swap(svm: &mut LiteSVM, signer: &Keypair, ix: solana_sdk::instruction::Instruction) {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&signer.pubkey()), &blockhash);
    let tx = Transaction::new(&[signer], msg, blockhash);
    let res = svm.send_transaction(tx);
    if let Err(e) = res {
        for l in &e.meta.logs { eprintln!("log: {}", l); }
        panic!("swap tx failed: {:?}", e.err);
    }
}

#[test]
fn sandwich_wsol_surge_profitable() {
    if skip_if_missing() {
        eprintln!("skipping: snapshot or .so missing");
        return;
    }

    let manifest = PoolManifest::read(&snapshot_dir().join("manifest.json")).unwrap();
    let pool = RaydiumCpmmPool::from_manifest(manifest).unwrap();

    let mut svm = LiteSVM::default()
        .with_builtins().with_default_programs().with_sysvars()
        .with_lamports(1_000_000_000_000)
        .with_sigverify(false).with_blockhash_check(false);
    load_raydium_cpmm_pool(&mut svm, &pool, &snapshot_dir(), &cpmm_so()).unwrap();

    let cfg_acc = svm.get_account(&pool.amm_config).unwrap();
    let cfg = AmmConfig::deserialize(&cfg_acc.data).expect("AmmConfig");
    println!("trade_fee={} creator_fee={} (denom 1e6)", cfg.trade_fee_rate, cfg.creator_fee_rate);

    // Actors
    let attacker = Keypair::new();
    let victim = Keypair::new();
    svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&victim.pubkey(), 10_000_000_000).unwrap();

    // Token accounts
    let att_wsol = Keypair::new().pubkey();
    let att_surge = Keypair::new().pubkey();
    let vic_wsol = Keypair::new().pubkey();
    let vic_surge = Keypair::new().pubkey();

    // Sizing — pool has ~2106 WSOL reserve. Frontrun 5 SOL (~0.24% of pool),
    // victim 10 SOL (~0.47% of pool). Realistic retail sandwich ratio.
    let frontrun_in: u64 = 5_000_000_000;
    let victim_in: u64 = 10_000_000_000;

    fund_token_account(&mut svm, &att_wsol, &pool.mint_a, &attacker.pubkey(), frontrun_in).unwrap();
    fund_token_account(&mut svm, &att_surge, &pool.mint_b, &attacker.pubkey(), 0).unwrap();
    fund_token_account(&mut svm, &vic_wsol, &pool.mint_a, &victim.pubkey(), victim_in).unwrap();
    fund_token_account(&mut svm, &vic_surge, &pool.mint_b, &victim.pubkey(), 0).unwrap();

    let program_id = raydium_cpmm_program_pubkey();
    let vault_a_pre = read_token_amount(&svm, &pool.vault_a);
    let vault_b_pre = read_token_amount(&svm, &pool.vault_b);
    println!("INITIAL pool: vault_a={} vault_b={}", vault_a_pre, vault_b_pre);

    // === 1. FRONTRUN: attacker WSOL → SURGE ===
    let ix = build_swap(&pool, program_id, &attacker.pubkey(), &att_wsol, &att_surge, true, frontrun_in);
    send_swap(&mut svm, &attacker, ix);
    let att_surge_after_front = read_token_amount(&svm, &att_surge);
    println!(
        "FRONTRUN: attacker {} WSOL → {} SURGE   (vault_a={} vault_b={})",
        frontrun_in, att_surge_after_front,
        read_token_amount(&svm, &pool.vault_a), read_token_amount(&svm, &pool.vault_b),
    );

    // === 2. VICTIM: same direction ===
    let ix = build_swap(&pool, program_id, &victim.pubkey(), &vic_wsol, &vic_surge, true, victim_in);
    send_swap(&mut svm, &victim, ix);
    let vic_surge_recv = read_token_amount(&svm, &vic_surge);
    println!(
        "VICTIM:   victim   {} WSOL → {} SURGE   (vault_a={} vault_b={})",
        victim_in, vic_surge_recv,
        read_token_amount(&svm, &pool.vault_a), read_token_amount(&svm, &pool.vault_b),
    );

    // What victim *would* have received without sandwich (using initial reserves)
    let fee_num = cfg.trade_fee_rate as u128;
    let denom = 1_000_000u128;
    let victim_in_after_fee = (victim_in as u128) - ((victim_in as u128) * fee_num).div_ceil(denom);
    let no_sandwich_recv = (vault_b_pre as u128) * victim_in_after_fee
        / ((vault_a_pre as u128) + victim_in_after_fee);
    let victim_loss = no_sandwich_recv - (vic_surge_recv as u128);
    println!(
        "VICTIM_LOSS (vs no-sandwich): {} SURGE ({:.4}%)",
        victim_loss,
        100.0 * victim_loss as f64 / no_sandwich_recv as f64
    );

    // === 3. BACKRUN: attacker dumps all received SURGE back to WSOL ===
    let ix = build_swap(&pool, program_id, &attacker.pubkey(), &att_surge, &att_wsol, false, att_surge_after_front);
    send_swap(&mut svm, &attacker, ix);
    let att_wsol_final = read_token_amount(&svm, &att_wsol);
    let att_surge_final = read_token_amount(&svm, &att_surge);
    println!(
        "BACKRUN:  attacker {} SURGE → {} WSOL   (vault_a={} vault_b={})",
        att_surge_after_front, att_wsol_final,
        read_token_amount(&svm, &pool.vault_a), read_token_amount(&svm, &pool.vault_b),
    );

    let gross_profit = att_wsol_final as i128 - frontrun_in as i128;
    let bps = (gross_profit * 10_000) / frontrun_in as i128;
    println!(
        "GROSS PROFIT: {} lamports ({:+} bps of frontrun_in)",
        gross_profit, bps
    );

    assert_eq!(att_surge_final, 0, "attacker dumped all SURGE");
    assert!(gross_profit > 0, "sandwich must be profitable (got {} lamports)", gross_profit);
    assert!(victim_loss > 0, "victim must be worse off than no-sandwich baseline");
}
