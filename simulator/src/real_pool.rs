//! Real-pool snapshot reader.
//!
//! Loads a Raydium CPMM pool snapshot produced by `fork::bin::snapshot` so the
//! simulator can run the sandwich grid against actual mainnet reserves
//! instead of synthetic values.
//!
//! Snapshot layout (see `fork/cache/pools/<label>/`):
//!   - `manifest.json`           — labels + pubkeys + slot
//!   - `<vault_a>.json`,
//!     `<vault_b>.json`          — SPL token account JSONs (base64 `data_b64`)
//!   - `<amm_config>.json`       — Raydium AmmConfig PDA, deserialized via
//!                                 `carbon_raydium_cpmm_decoder`.
//!
//! SPL Token Account v1 layout (165 bytes):
//!   bytes [ 0..32]  mint
//!   bytes [32..64]  owner
//!   bytes [64..72]  amount (u64 LE)   <-- vault reserve
//!   ... (remainder unused here)
//! Reference: spl-token Account struct, solana-program-library v3.5+.
//!
//! CPMM fee rates use a 1e6 denominator (0.25% == 2500). The existing sweep
//! grid stores `pool_fee_bps` with a 1e4 denominator, so we expose both: a
//! lossy `pool_fee_bps` (rounded) for backward compatibility with the
//! synthetic grid and the exact `trade_fee_rate` / `creator_fee_rate` for
//! future multi-fee math.
//!
//! `engine::run_single` consumes the multi-fee CPMM math
//! (`amm_math::multi_fee::compute_swap_multi_fee`) so on-chain fee splits
//! (trade + creator) are reproduced faithfully when a real pool is loaded.
//! The synthetic sweep regime constructs an equivalent single-fee
//! `MultiFeeConfig` (denom=10_000, creator disabled) — see
//! [`fee_config_from_pool`].
//!
//! TODO(creator_fee_on): the snapshot currently hardcodes `creator_fee_on_input
//! = true` (Raydium CPMM v1's typical wiring). To support `OnOutput` correctly
//! per pool we need to read `PoolState::creator_fee_on` at snapshot time and
//! plumb that into `RealPool`.

use std::fs;
use std::path::{Path, PathBuf};

use amm_math::multi_fee::{CreatorFeeMode, MultiFeeConfig};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use carbon_core::deserialize::CarbonDeserialize;
use carbon_raydium_cpmm_decoder::accounts::amm_config::AmmConfig;
use serde::Deserialize;

#[derive(Debug, Clone)]
#[allow(dead_code)] // Some fields are kept for future multi-fee math / labelling.
pub struct RealPool {
    pub label: String,
    pub reserve_a: u128,
    pub reserve_b: u128,
    /// CPMM trade fee numerator over 1e6 (e.g. 2500 = 0.25%).
    pub trade_fee_rate: u64,
    /// CPMM creator fee numerator over 1e6.
    pub creator_fee_rate: u64,
    /// Raydium CPMM v1 always charges the creator fee on the input side.
    /// Kept as a field so other implementations / future versions can flip it.
    pub creator_fee_on_input: bool,
    pub mint_a: String,
    pub mint_b: String,
    pub snapshot_slot: u64,
    pub snapshot_unix_ts: i64,
}

impl RealPool {
    /// Approximate the Raydium fee as a basis-points value (denom 1e4).
    /// Lossy: 2500/1e6 (0.25%) -> 25 bps. Caller should prefer
    /// `trade_fee_rate` for accurate math.
    pub fn pool_fee_bps_approx(&self) -> u16 {
        // 1e6 denom -> 1e4 denom: divide by 100, rounded.
        let bps = (self.trade_fee_rate + 50) / 100;
        bps.min(u16::MAX as u64) as u16
    }
}

/// Build a `MultiFeeConfig` that drives the swap math.
///
/// Two regimes:
/// - **Synthetic sweep** (`real = None`): trade-fee-only, denom=`10_000` (bps),
///   creator fee disabled. Numerically equivalent to legacy `compute_swap`
///   (modulo a 1-unit ceil-vs-floor rounding diff on the input fee).
/// - **Real-pool replay** (`real = Some(_)`): trade + creator numerators over
///   `1_000_000`, with `creator_fee_mode` derived from
///   `RealPool::creator_fee_on_input`.
pub fn fee_config_from_pool(pool_fee_bps: u16, real: Option<&RealPool>) -> MultiFeeConfig {
    match real {
        Some(rp) => MultiFeeConfig {
            trade_fee_rate: rp.trade_fee_rate,
            creator_fee_rate: rp.creator_fee_rate,
            fee_denominator: 1_000_000,
            creator_fee_mode: if rp.creator_fee_rate == 0 {
                CreatorFeeMode::Disabled
            } else if rp.creator_fee_on_input {
                CreatorFeeMode::OnInput
            } else {
                CreatorFeeMode::OnOutput
            },
        },
        None => MultiFeeConfig::single(pool_fee_bps as u64, 10_000),
    }
}

#[derive(Debug, Deserialize)]
struct PoolManifestJson {
    label: String,
    #[allow(dead_code)]
    program_id: String,
    #[allow(dead_code)]
    pool_address: String,
    #[serde(default)]
    snapshot_slot: u64,
    #[serde(default)]
    snapshot_unix_ts: i64,
    mint_a: String,
    mint_b: String,
    vault_a: String,
    vault_b: String,
    amm_config: String,
}

#[derive(Debug, Deserialize)]
struct CachedAccountJson {
    #[allow(dead_code)]
    pubkey: String,
    data_b64: String,
    #[allow(dead_code)]
    #[serde(default)]
    owner: String,
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error, PathBuf),
    Json(serde_json::Error, PathBuf),
    Base64(base64::DecodeError, PathBuf),
    VaultTooShort { path: PathBuf, len: usize },
    AmmConfigDeser { path: PathBuf },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e, p) => write!(f, "I/O error reading {}: {}", p.display(), e),
            Self::Json(e, p) => write!(f, "JSON parse error in {}: {}", p.display(), e),
            Self::Base64(e, p) => write!(f, "base64 decode error in {}: {}", p.display(), e),
            Self::VaultTooShort { path, len } => write!(
                f,
                "vault data in {} is {} bytes, need >= 72 (SPL Token Account amount @ offset 64)",
                path.display(),
                len
            ),
            Self::AmmConfigDeser { path } => {
                write!(f, "failed to deserialize AmmConfig from {}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, LoadError> {
    let bytes = fs::read(path).map_err(|e| LoadError::Io(e, path.to_path_buf()))?;
    serde_json::from_slice(&bytes).map_err(|e| LoadError::Json(e, path.to_path_buf()))
}

fn read_vault_amount(path: &Path) -> Result<u64, LoadError> {
    let acc: CachedAccountJson = read_json(path)?;
    let data = B64
        .decode(acc.data_b64.as_bytes())
        .map_err(|e| LoadError::Base64(e, path.to_path_buf()))?;
    if data.len() < 72 {
        return Err(LoadError::VaultTooShort {
            path: path.to_path_buf(),
            len: data.len(),
        });
    }
    let mut amt = [0u8; 8];
    amt.copy_from_slice(&data[64..72]);
    Ok(u64::from_le_bytes(amt))
}

/// Load a real-pool snapshot from `<snapshot_dir>/manifest.json` and the
/// referenced cached account JSONs.
pub fn load_real_pool(snapshot_dir: &Path) -> Result<RealPool, LoadError> {
    let manifest_path = snapshot_dir.join("manifest.json");
    let m: PoolManifestJson = read_json(&manifest_path)?;

    let reserve_a = read_vault_amount(&snapshot_dir.join(format!("{}.json", m.vault_a)))? as u128;
    let reserve_b = read_vault_amount(&snapshot_dir.join(format!("{}.json", m.vault_b)))? as u128;

    let cfg_path = snapshot_dir.join(format!("{}.json", m.amm_config));
    let cfg_acc: CachedAccountJson = read_json(&cfg_path)?;
    let cfg_data = B64
        .decode(cfg_acc.data_b64.as_bytes())
        .map_err(|e| LoadError::Base64(e, cfg_path.clone()))?;
    let cfg = <AmmConfig as CarbonDeserialize>::deserialize(&cfg_data)
        .ok_or_else(|| LoadError::AmmConfigDeser { path: cfg_path })?;

    Ok(RealPool {
        label: m.label,
        reserve_a,
        reserve_b,
        trade_fee_rate: cfg.trade_fee_rate,
        creator_fee_rate: cfg.creator_fee_rate,
        creator_fee_on_input: true,
        mint_a: m.mint_a,
        mint_b: m.mint_b,
        snapshot_slot: m.snapshot_slot,
        snapshot_unix_ts: m.snapshot_unix_ts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_root() -> PathBuf {
        // simulator/ is a workspace member at the workspace root.
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .to_path_buf()
    }

    #[test]
    fn loads_wsol_surge_snapshot() {
        let dir = workspace_root().join("fork/cache/pools/wsol_surge");
        if !dir.join("manifest.json").exists() {
            eprintln!("skipping: snapshot not present at {}", dir.display());
            return;
        }
        let p = load_real_pool(&dir).expect("load wsol_surge");
        assert!(p.reserve_a > 0, "reserve_a > 0");
        assert!(p.reserve_b > 0, "reserve_b > 0");
        assert_eq!(p.trade_fee_rate, 2500, "expected 0.25% trade fee");
        assert_eq!(p.creator_fee_rate, 500, "expected 0.05% creator fee");
        assert_eq!(
            p.mint_a, "So11111111111111111111111111111111111111112",
            "mint_a should be WSOL"
        );
        assert!(p.snapshot_slot > 0, "snapshot_slot > 0");
        eprintln!(
            "wsol_surge: reserve_a={} reserve_b={} trade_fee={} creator_fee={}",
            p.reserve_a, p.reserve_b, p.trade_fee_rate, p.creator_fee_rate
        );
    }

    /// Regime A backwards-compat: `fee_config_from_pool(30, None)` produces a
    /// `MultiFeeConfig` whose output matches the legacy single-fee
    /// `compute_swap` to within 1 unit (ceil-on-fee vs floor-on-fee shifts
    /// the result by at most a single wei). Source for the 30 bps Uniswap-V2
    /// fee: Adams et al. 2020, §2.4.
    #[test]
    fn synthetic_regime_matches_legacy_single_fee() {
        use amm_math::constant_product::compute_swap;
        use amm_math::multi_fee::compute_swap_multi_fee;

        let cfg = fee_config_from_pool(30, None);
        assert_eq!(cfg.fee_denominator, 10_000);
        assert_eq!(cfg.trade_fee_rate, 30);
        assert_eq!(cfg.creator_fee_rate, 0);

        for (r_in, r_out, amt) in [
            (1_000_000u128, 1_000_000u128, 10_000u128),
            (10_000_000u128, 5_000_000u128, 250_000u128),
            (
                2_106_428_125_817u128,
                1_756_035_099_685_335u128,
                1_000_000_000u128,
            ),
        ] {
            let multi = compute_swap_multi_fee(r_in, r_out, amt, &cfg)
                .expect("multi-fee swap")
                .amount_out;
            let legacy = compute_swap(amt, r_in, r_out, 30)
                .expect("legacy swap")
                .amount_out;
            let diff = if multi > legacy {
                multi - legacy
            } else {
                legacy - multi
            };
            assert!(diff <= 1, "multi={multi} legacy={legacy}");
        }
    }

    /// Regime A: a bps-denominated config and an equivalent micro-denominator
    /// config (3000/1e6 = 30 bps) must produce the same output (mod 1 unit).
    #[test]
    fn fee_config_bps_and_micro_denominator_agree() {
        use amm_math::multi_fee::{compute_swap_multi_fee, MultiFeeConfig};
        let bps = fee_config_from_pool(30, None);
        let micro = MultiFeeConfig::single(3000, 1_000_000);
        let r_in = 1_000_000u128;
        let r_out = 1_000_000u128;
        let amt = 10_000u128;
        let a = compute_swap_multi_fee(r_in, r_out, amt, &bps)
            .expect("bps swap")
            .amount_out;
        let b = compute_swap_multi_fee(r_in, r_out, amt, &micro)
            .expect("micro swap")
            .amount_out;
        let diff = if a > b { a - b } else { b - a };
        assert!(diff <= 1);
    }

    /// Regime B: real-pool replay through the new path. Pinned against the
    /// `replays_swap_e2e_smoke_test_on_input` deterministic value in
    /// `crates/amm-math/src/cpmm/multi_fee.rs` (trade=2500/1e6 + creator=500/1e6
    /// on input). Tolerance: equality, since the math is deterministic.
    #[test]
    fn real_pool_regime_uses_creator_fee_on_input() {
        use amm_math::multi_fee::{compute_swap_multi_fee, CreatorFeeMode};

        let rp = RealPool {
            label: "wsol_surge".into(),
            reserve_a: 2_106_428_125_817,
            reserve_b: 1_756_035_099_685_335,
            trade_fee_rate: 2500,
            creator_fee_rate: 500,
            creator_fee_on_input: true,
            mint_a: "So11111111111111111111111111111111111111112".into(),
            mint_b: "".into(),
            snapshot_slot: 0,
            snapshot_unix_ts: 0,
        };
        let cfg = fee_config_from_pool(rp.pool_fee_bps_approx(), Some(&rp));
        assert_eq!(cfg.fee_denominator, 1_000_000);
        assert_eq!(cfg.trade_fee_rate, 2500);
        assert_eq!(cfg.creator_fee_rate, 500);
        assert!(matches!(cfg.creator_fee_mode, CreatorFeeMode::OnInput));

        let out = compute_swap_multi_fee(rp.reserve_a, rp.reserve_b, 1_000_000_000u128, &cfg)
            .expect("real-pool swap")
            .amount_out;
        assert_eq!(out, 830_761_184_793u128);
    }

    #[test]
    fn missing_manifest_returns_error_not_panic() {
        let dir = workspace_root().join("fork/cache/pools/__does_not_exist__");
        let res = load_real_pool(&dir);
        assert!(res.is_err(), "expected Err, got {:?}", res);
    }
}
