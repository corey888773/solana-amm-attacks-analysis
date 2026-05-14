use anyhow::{Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_commitment_config::CommitmentConfig;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_status_client_types::{
    EncodedConfirmedTransactionWithStatusMeta, UiTransactionEncoding, UiTransactionStatusMeta,
};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn transaction_cache_path(cache_root: &Path, signature: &str) -> PathBuf {
    cache_root
        .join("transactions")
        .join(format!("{signature}.json"))
}

pub fn fetch_and_cache_transaction(
    rpc: &RpcClient,
    cache_root: &Path,
    signature: &str,
) -> Result<EncodedConfirmedTransactionWithStatusMeta> {
    let path = transaction_cache_path(cache_root, signature);
    if path.exists() {
        return read_cached_transaction(&path);
    }

    let sig = Signature::from_str(signature).context("parse signature")?;
    let cfg = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        commitment: Some(CommitmentConfig::finalized()),
        max_supported_transaction_version: Some(0),
    };
    // Retry transient RPC failures (connection drops, timeouts, 429s) with
    // exponential backoff. 6 attempts ≈ up to ~63s of waiting before giving up.
    let mut attempt = 0u32;
    let tx = loop {
        match rpc.get_transaction_with_config(&sig, cfg.clone()) {
            Ok(tx) => break tx,
            Err(e) if attempt < 5 => {
                let backoff_ms = 500u64 << attempt; // 0.5s, 1s, 2s, 4s, 8s, 16s
                eprintln!(
                    "rpc retry {}/5 for {signature} after {}ms: {e}",
                    attempt + 1,
                    backoff_ms
                );
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                attempt += 1;
            }
            Err(e) => {
                return Err(anyhow::Error::new(e))
                    .with_context(|| format!("get transaction {signature} after 6 attempts"));
            }
        }
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(&tx)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(tx)
}

pub fn read_cached_transaction(path: &Path) -> Result<EncodedConfirmedTransactionWithStatusMeta> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("decode {}", path.display()))
}

pub fn decode_transaction(
    tx: &EncodedConfirmedTransactionWithStatusMeta,
) -> Result<VersionedTransaction> {
    tx.transaction
        .transaction
        .decode()
        .context("decode base64 transaction")
}

pub fn transaction_meta(
    tx: &EncodedConfirmedTransactionWithStatusMeta,
) -> Result<&UiTransactionStatusMeta> {
    tx.transaction
        .meta
        .as_ref()
        .context("missing transaction meta")
}
