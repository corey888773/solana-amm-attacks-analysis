use crate::historical_cpmm::artifacts::SignatureRow;
use crate::historical_cpmm::config::PoolTarget;
use anyhow::{Context, Result};
use solana_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_commitment_config::CommitmentConfig;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// Solana JSON-RPC `getSignaturesForAddress` returns at most 1000 entries per
/// call, regardless of requested `limit`. Stay one below that to avoid
/// edge-case parser issues seen on some validators.
const RPC_PAGE_SIZE: usize = 1000;

/// Collect signatures for each pool. If `min_block_time` is `Some(unix)`,
/// the collector paginates backwards (using `before=last_sig`) until either
/// (a) the next page is empty, (b) any signature in the page is older than
/// `min_block_time`, or (c) `limit_per_pool` is reached. With
/// `min_block_time = None` the legacy single-call behavior is preserved
/// (limit_per_pool capped at 1000).
pub fn collect_signatures(
    rpc: &RpcClient,
    pools: &[PoolTarget],
    limit_per_pool: usize,
    min_block_time: Option<i64>,
) -> Result<Vec<SignatureRow>> {
    let collected_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut rows = Vec::new();

    for pool in pools {
        let pool_pubkey = pool.pubkey()?;
        let mut before: Option<solana_signature::Signature> = None;
        let mut collected_for_pool: usize = 0;

        loop {
            let remaining = limit_per_pool.saturating_sub(collected_for_pool);
            if remaining == 0 {
                break;
            }
            let page_limit = remaining.min(RPC_PAGE_SIZE);
            let page = rpc
                .get_signatures_for_address_with_config(
                    &pool_pubkey,
                    GetConfirmedSignaturesForAddress2Config {
                        before,
                        until: None,
                        limit: Some(page_limit),
                        commitment: Some(CommitmentConfig::finalized()),
                    },
                )
                .with_context(|| format!("get signatures for {}", pool.label))?;

            if page.is_empty() {
                break;
            }

            // Track the last signature in the page to use as the next `before`.
            let last_sig_str = page.last().map(|s| s.signature.clone());

            let mut hit_floor = false;
            for sig in page {
                if let (Some(min_ts), Some(bt)) = (min_block_time, sig.block_time) {
                    if bt < min_ts {
                        hit_floor = true;
                        break;
                    }
                }
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
                collected_for_pool += 1;
                if collected_for_pool >= limit_per_pool {
                    break;
                }
            }

            if hit_floor || collected_for_pool >= limit_per_pool {
                break;
            }
            // No floor configured and the page came back smaller than asked
            // for: RPC has nothing older.
            let Some(next_before_str) = last_sig_str else { break };
            before = Some(
                solana_signature::Signature::from_str(&next_before_str)
                    .with_context(|| format!("parse signature {next_before_str}"))?,
            );
        }
    }

    Ok(rows)
}
