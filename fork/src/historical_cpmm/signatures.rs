use crate::historical_cpmm::artifacts::SignatureRow;
use crate::historical_cpmm::config::PoolTarget;
use anyhow::{Context, Result};
use solana_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_commitment_config::CommitmentConfig;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn collect_signatures(
    rpc: &RpcClient,
    pools: &[PoolTarget],
    limit_per_pool: usize,
) -> Result<Vec<SignatureRow>> {
    let collected_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut rows = Vec::new();

    for pool in pools {
        let pool_pubkey = pool.pubkey()?;
        let signatures = rpc
            .get_signatures_for_address_with_config(
                &pool_pubkey,
                GetConfirmedSignaturesForAddress2Config {
                    before: None,
                    until: None,
                    limit: Some(limit_per_pool),
                    commitment: Some(CommitmentConfig::finalized()),
                },
            )
            .with_context(|| format!("get signatures for {}", pool.label))?;

        for sig in signatures {
            rows.push(SignatureRow {
                pool_label: pool.label.clone(),
                pool_address: pool.address.clone(),
                signature: sig.signature,
                slot: sig.slot,
                block_time: sig.block_time,
                err: sig.err.map(|err| format!("{err:?}")),
                source_account: pool.address.clone(),
                collected_at_unix,
            });
        }
    }

    Ok(rows)
}
