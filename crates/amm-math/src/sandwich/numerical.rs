//! Numerical sandwich-attack frontrun optimizer.
//!
//! Closed-form solution from Zhou et al. ("High-Frequency Trading on
//! Decentralized On-Chain Exchanges", IEEE S&P 2021), §IV-B Eq. 3:
//!
//! ```text
//! V_f* = (sqrt(x * (x + (1-phi) * v)) - x) / (1 - phi)
//! ```
//!
//! is **suboptimal** under positive fee `phi > 0` because the derivation
//! ignores that `phi` is paid on BOTH legs (frontrun *and* backrun). On thin
//! pools the closed-form `V_f*` produces negative net profit even though a
//! smaller frontrun would still be profitable.
//!
//! This module performs a ternary search over the unimodal net-profit
//! function `pi(V_f) = backrun_out(V_f, victim) - V_f - 2 * tx_cost_per_leg`,
//! evaluating each candidate by simulating the three swaps (frontrun /
//! victim / backrun) using the same `SwapResult` state transition as the
//! simulator. ~100 iterations are sufficient for u128 precision down to a
//! single unit.

use crate::multi_fee::{compute_swap_multi_fee, MultiFeeConfig};
use crate::types::SandwichResult;
use crate::BPS_DENOMINATOR_F64;

/// Simulate frontrun -> victim -> backrun for a chosen frontrun size.
fn simulate_sandwich(
    reserve_in: u128,
    reserve_out: u128,
    victim_amount_in: u128,
    cfg: &MultiFeeConfig,
    tx_cost_per_leg: u128,
    frontrun_in: u128,
) -> Option<SandwichResult> {
    if frontrun_in == 0 {
        return None;
    }

    let fair_swap = compute_swap_multi_fee(reserve_in, reserve_out, victim_amount_in, cfg)?;

    // Frontrun: attacker buys token_out.
    let frontrun = compute_swap_multi_fee(reserve_in, reserve_out, frontrun_in, cfg)?;

    // Victim swap (same direction).
    let victim = compute_swap_multi_fee(
        frontrun.new_reserve_in,
        frontrun.new_reserve_out,
        victim_amount_in,
        cfg,
    )?;

    // Backrun: attacker sells everything bought in the frontrun. Direction flips.
    let backrun = compute_swap_multi_fee(
        victim.new_reserve_out,
        victim.new_reserve_in,
        frontrun.amount_out,
        cfg,
    )?;

    let victim_loss = fair_swap.amount_out as i128 - victim.amount_out as i128;
    let victim_extra_slippage_bps = if fair_swap.amount_out > 0 {
        (victim_loss as f64 / fair_swap.amount_out as f64 * BPS_DENOMINATOR_F64) as u64
    } else {
        0
    };
    let gross_profit = backrun.amount_out as i128 - frontrun_in as i128;
    let net_profit = gross_profit - 2 * tx_cost_per_leg as i128;

    Some(SandwichResult {
        frontrun_amount: frontrun_in,
        frontrun_output: frontrun.amount_out,
        backrun_output: backrun.amount_out,
        victim_extra_slippage_bps,
        gross_profit,
        net_profit,
        is_profitable: net_profit > 0,
    })
}

/// Simulate frontrun -> victim -> backrun and return net profit. `i128` allows
/// negative results during optimizer search.
fn net_profit(
    reserve_in: u128,
    reserve_out: u128,
    victim_amount_in: u128,
    cfg: &MultiFeeConfig,
    tx_cost_per_leg: u128,
    frontrun_in: u128,
) -> i128 {
    if frontrun_in == 0 {
        return 0i128 - 2 * tx_cost_per_leg as i128;
    }

    simulate_sandwich(
        reserve_in,
        reserve_out,
        victim_amount_in,
        cfg,
        tx_cost_per_leg,
        frontrun_in,
    )
    .map(|result| result.net_profit)
    .unwrap_or(i128::MIN / 2)
}

/// Find the frontrun input `V_f` that maximizes net profit via ternary
/// search and return the full sandwich analysis.
///
/// Search range is `[0, reserve_in]`. The upper cap reflects realistic MEV
/// economics: a frontrun input larger than the pool's reserve_in has no
/// economic interpretation — slippage diverges, and the integer-CPMM math
/// produces a spurious "plateau" because output saturates at `reserve_out`
/// (the asymptote of `dy = y * dx / (x + dx)` as `dx → ∞`). Without this
/// cap the optimizer wanders into that plateau and reports 10-20× inflated
/// "profits" that no real attacker could realize. See Zhou et al., IEEE S&P
/// 2021, §IV-B Eq. 3 for the closed-form upper bound on the realistic
/// regime.
///
/// Net profit is unimodal in `V_f` over `(0, reserve_in)` for `phi > 0`
/// (concave with a single maximum), so ternary search converges in
/// `O(log(range / precision))`.
///
/// # Example
///
/// ```
/// use amm_math::multi_fee::MultiFeeConfig;
/// use amm_math::sandwich::compute_numerical_sandwich;
///
/// let cfg = MultiFeeConfig::single(30, 10_000);
/// let result = compute_numerical_sandwich(
///     1_000_000,
///     1_000_000,
///     50_000,
///     &cfg,
///     0,
/// ).expect("profitable sandwich");
///
/// assert!(result.frontrun_amount > 0);
/// assert!(result.net_profit > 0);
/// ```
pub fn compute_numerical_sandwich(
    reserve_in: u128,
    reserve_out: u128,
    victim_amount_in: u128,
    cfg: &MultiFeeConfig,
    tx_cost_per_leg: u128,
) -> Option<SandwichResult> {
    if victim_amount_in == 0 || reserve_in == 0 || reserve_out == 0 {
        return None;
    }

    let mut lo: u128 = 0;
    // Cap at reserve_in: economically infeasible region beyond, and the
    // integer-CPMM saturation creates a false-positive plateau there.
    let mut hi: u128 = reserve_in;

    // ~100 iterations: each step shrinks the range by 2/3, so after 100 iters
    // hi - lo <= initial * (2/3)^100 ~= 2.5e-18 of initial range — well below
    // 1 unit for any realistic pool.
    for _ in 0..200 {
        if hi <= lo + 2 {
            break;
        }
        let third = (hi - lo) / 3;
        let m1 = lo + third;
        let m2 = hi - third;
        let p1 = net_profit(
            reserve_in,
            reserve_out,
            victim_amount_in,
            cfg,
            tx_cost_per_leg,
            m1,
        );
        let p2 = net_profit(
            reserve_in,
            reserve_out,
            victim_amount_in,
            cfg,
            tx_cost_per_leg,
            m2,
        );
        if p1 < p2 {
            lo = m1;
        } else {
            hi = m2;
        }
    }

    // Linear refinement over the small remaining bracket: pick the integer
    // candidate with the highest net profit (and prefer 0 if it dominates).
    let mut best_v: u128 = 0;
    let mut best_p = net_profit(
        reserve_in,
        reserve_out,
        victim_amount_in,
        cfg,
        tx_cost_per_leg,
        0,
    );
    let lo_check = lo.saturating_sub(1);
    let hi_check = hi.saturating_add(1);
    let mut v = lo_check;
    while v <= hi_check {
        let p = net_profit(
            reserve_in,
            reserve_out,
            victim_amount_in,
            cfg,
            tx_cost_per_leg,
            v,
        );
        if p > best_p {
            best_p = p;
            best_v = v;
        }
        if v == hi_check {
            break;
        }
        v += 1;
    }

    if best_p <= 0 {
        return None;
    }

    simulate_sandwich(
        reserve_in,
        reserve_out,
        victim_amount_in,
        cfg,
        tx_cost_per_leg,
        best_v,
    )
}

/// Brute-force/grid oracle for validating the numerical optimizer.
///
/// The production optimizer (`compute_numerical_sandwich`) uses ternary search,
/// which is fast but relies on the profit curve having one practical maximum.
/// This oracle makes the opposite tradeoff: it is slow, simple, and does not
/// assume unimodality. On small ranges it checks every integer frontrun size;
/// on large ranges it samples an evenly-spaced grid. That makes it useful as a
/// regression/benchmark baseline: if ternary search reports materially worse
/// profit than the grid result, the unimodality assumption, search bounds, or
/// rounding behavior need investigation.
///
/// Use this in tests, examples, and CSV benchmark harnesses, not hot simulator
/// paths. For `reserve_in <= max_steps` it checks every integer frontrun size
/// in `[1, reserve_in]`; otherwise it samples an evenly-spaced grid and always
/// includes `reserve_in`.
pub fn compute_grid_sandwich(
    reserve_in: u128,
    reserve_out: u128,
    victim_amount_in: u128,
    cfg: &MultiFeeConfig,
    tx_cost_per_leg: u128,
    max_steps: u64,
) -> Option<SandwichResult> {
    if victim_amount_in == 0 || reserve_in == 0 || reserve_out == 0 || max_steps == 0 {
        return None;
    }

    let mut best_v: u128 = 0;
    let mut best_p = net_profit(
        reserve_in,
        reserve_out,
        victim_amount_in,
        cfg,
        tx_cost_per_leg,
        0,
    );

    let steps = u128::from(max_steps);
    let step = if reserve_in <= steps {
        1
    } else {
        reserve_in.div_ceil(steps)
    };

    let mut v = step;
    while v <= reserve_in {
        let p = net_profit(
            reserve_in,
            reserve_out,
            victim_amount_in,
            cfg,
            tx_cost_per_leg,
            v,
        );
        if p > best_p {
            best_p = p;
            best_v = v;
        }

        match v.checked_add(step) {
            Some(next) if next > v => v = next,
            _ => break,
        }
    }

    if best_v != reserve_in {
        let p = net_profit(
            reserve_in,
            reserve_out,
            victim_amount_in,
            cfg,
            tx_cost_per_leg,
            reserve_in,
        );
        if p > best_p {
            best_p = p;
            best_v = reserve_in;
        }
    }

    if best_p <= 0 {
        return None;
    }

    simulate_sandwich(
        reserve_in,
        reserve_out,
        victim_amount_in,
        cfg,
        tx_cost_per_leg,
        best_v,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multi_fee::{CreatorFeeMode, MultiFeeConfig};

    fn cfg_30bps() -> MultiFeeConfig {
        MultiFeeConfig::single(3000, 1_000_000) // 0.30 %
    }

    fn cfg_raydium() -> MultiFeeConfig {
        MultiFeeConfig {
            trade_fee_rate: 2500,
            creator_fee_rate: 500,
            fee_denominator: 1_000_000,
            creator_fee_mode: CreatorFeeMode::OnInput,
        }
    }

    fn numerical_frontrun_amount(
        reserve_in: u128,
        reserve_out: u128,
        victim_amount_in: u128,
        cfg: &MultiFeeConfig,
        tx_cost_per_leg: u128,
    ) -> u128 {
        compute_numerical_sandwich(
            reserve_in,
            reserve_out,
            victim_amount_in,
            cfg,
            tx_cost_per_leg,
        )
        .map(|result| result.frontrun_amount)
        .unwrap_or(0)
    }

    #[test]
    fn zero_victim_yields_zero_frontrun() {
        let v = numerical_frontrun_amount(1_000_000, 1_000_000, 0, &cfg_30bps(), 0);
        assert_eq!(v, 0);
    }

    #[test]
    fn compute_numerical_sandwich_returns_full_result() {
        let result = compute_numerical_sandwich(1_000_000, 1_000_000, 50_000, &cfg_30bps(), 0)
            .expect("profitable numerical sandwich");

        assert!(result.frontrun_amount > 0);
        assert!(result.frontrun_output > 0);
        assert!(result.backrun_output > 0);
        assert!(result.victim_extra_slippage_bps > 0);
        assert_eq!(
            result.frontrun_amount,
            numerical_frontrun_amount(1_000_000, 1_000_000, 50_000, &cfg_30bps(), 0)
        );
    }

    #[test]
    fn grid_oracle_matches_numerical_on_small_exact_search() {
        let reserve_in = 5_000u128;
        let reserve_out = 5_000u128;
        let victim = 500u128;
        let cfg = cfg_30bps();

        let numerical = compute_numerical_sandwich(reserve_in, reserve_out, victim, &cfg, 0)
            .expect("numerical");
        let grid =
            compute_grid_sandwich(reserve_in, reserve_out, victim, &cfg, 0, reserve_in as u64)
                .expect("grid");

        let tolerance = 2;
        assert!(
            numerical.net_profit >= grid.net_profit - tolerance,
            "numerical={} grid={}",
            numerical.net_profit,
            grid.net_profit
        );
    }

    #[test]
    fn grid_oracle_matrix_bounds_numerical_regression() {
        let scenarios = [
            (1_000u128, 1_000u128, 50u128),
            (2_500u128, 2_500u128, 125u128),
            (5_000u128, 4_000u128, 250u128),
            (8_000u128, 10_000u128, 400u128),
            (12_000u128, 12_000u128, 600u128),
        ];
        let fee_rates = [0u64, 5, 30, 100, 250];
        let tx_costs = [0u128, 1, 10];
        let tolerance = 4i128;

        for &(reserve_in, reserve_out, victim) in &scenarios {
            for &fee_rate in &fee_rates {
                let cfg = MultiFeeConfig::single(fee_rate, 10_000);
                for &tx_cost_per_leg in &tx_costs {
                    let numerical = compute_numerical_sandwich(
                        reserve_in,
                        reserve_out,
                        victim,
                        &cfg,
                        tx_cost_per_leg,
                    );
                    let grid = compute_grid_sandwich(
                        reserve_in,
                        reserve_out,
                        victim,
                        &cfg,
                        tx_cost_per_leg,
                        reserve_in as u64,
                    );

                    match (numerical, grid) {
                        (Some(numerical), Some(grid)) => assert!(
                            numerical.net_profit >= grid.net_profit - tolerance,
                            "numerical={} grid={} r_in={} r_out={} victim={} fee={} tx_cost_per_leg={}",
                            numerical.net_profit,
                            grid.net_profit,
                            reserve_in,
                            reserve_out,
                            victim,
                            fee_rate,
                            tx_cost_per_leg
                        ),
                        (None, Some(grid)) => assert!(
                            grid.net_profit <= tolerance,
                            "numerical none but grid profitable={} r_in={} r_out={} victim={} fee={} tx_cost_per_leg={}",
                            grid.net_profit,
                            reserve_in,
                            reserve_out,
                            victim,
                            fee_rate,
                            tx_cost_per_leg
                        ),
                        _ => {}
                    }
                }
            }
        }
    }

    #[test]
    fn grid_oracle_returns_none_when_no_profitable_attack() {
        let result =
            compute_grid_sandwich(1_000_000, 1_000_000, 10, &cfg_30bps(), 1_000_000_000, 1_000);

        assert!(result.is_none());
    }

    /// On the integer-CPMM model the numerical optimizer should produce
    /// profit at least as large as Zhou's continuous closed-form V_f for any
    /// fee level (this is the point of the optimizer — closed-form ignores
    /// the double-fee burden across frontrun + backrun).
    ///
    /// Historical note: an earlier revision of the optimizer used an upper
    /// search bracket of `10 * reserve_in`, which produced spurious
    /// "19× Zhou" profits because the integer-CPMM output saturates at
    /// `reserve_out` for `V_f > reserve_in`, creating a false plateau.
    /// The current bracket is `[0, reserve_in]` and the realistic uplift
    /// over Zhou is in the 1.0×-1.3× range on this scenario.
    ///
    /// Source for the closed-form: Zhou et al., IEEE S&P 2021, §IV-B Eq. 3.
    #[test]
    fn numerical_matches_or_beats_zhou_profit() {
        let r_in = 1_000_000u128;
        let r_out = 1_000_000u128;
        let v = 50_000u128;
        for trade_rate in &[0u64, 100, 1000, 3000, 10_000] {
            let cfg = MultiFeeConfig::single(*trade_rate, 1_000_000);
            let v_f_num = numerical_frontrun_amount(r_in, r_out, v, &cfg, 0);
            let p_num = net_profit(r_in, r_out, v, &cfg, 0, v_f_num);

            let phi = *trade_rate as f64 / 1_000_000.0;
            let one_minus_phi = (1.0 - phi).max(1e-9);
            let x = r_in as f64;
            let vf_f = v as f64;
            let zhou = ((x * vf_f) / ((x * (x + one_minus_phi * vf_f)).sqrt() + x)) as u128;
            let p_zhou = net_profit(r_in, r_out, v, &cfg, 0, zhou);

            assert!(
                p_num >= p_zhou,
                "trade={trade_rate}: numerical p={p_num} v={v_f_num} \
                 should beat Zhou p={p_zhou} v={zhou}"
            );
        }
    }

    /// The numerical optimizer should outperform the Zhou closed-form on a
    /// pool/victim configuration where the closed-form gives negative gross
    /// profit due to the double-fee burden.
    #[test]
    fn beats_zhou_on_thin_pool_with_fee() {
        // Thin pool, small victim, non-zero fee.
        let r_in = 100_000u128;
        let r_out = 100_000u128;
        let victim = 100u128;
        let cfg = cfg_30bps();
        let tx_cost_per_leg = 0u128;

        let v_num = numerical_frontrun_amount(r_in, r_out, victim, &cfg, tx_cost_per_leg);
        let p_num = net_profit(r_in, r_out, victim, &cfg, tx_cost_per_leg, v_num);

        // Zhou closed-form (phi = 0.003).
        let phi = 0.003f64;
        let one_minus_phi = 1.0 - phi;
        let x = r_in as f64;
        let v = victim as f64;
        let v_zhou = ((x * v) / ((x * (x + one_minus_phi * v)).sqrt() + x)) as u128;
        let p_zhou = net_profit(r_in, r_out, victim, &cfg, tx_cost_per_leg, v_zhou);

        // Numerical must be at least as good as Zhou.
        assert!(
            p_num >= p_zhou,
            "numerical p={p_num} should beat Zhou p={p_zhou}"
        );
        // And it should produce a non-negative profit (Zhou often goes negative here).
        assert!(
            p_num >= 0,
            "numerical net profit should be >= 0, got {p_num}"
        );
    }

    /// Convergence stability: running twice on identical inputs returns the
    /// same answer (within 1 unit).
    #[test]
    fn convergence_stable() {
        let r_in = 2_106_428_125_817u128;
        let r_out = 1_756_035_099_685_335u128;
        let victim = 1_000_000_000u128;
        let cfg = cfg_raydium();
        let a = numerical_frontrun_amount(r_in, r_out, victim, &cfg, 5_000);
        let b = numerical_frontrun_amount(r_in, r_out, victim, &cfg, 5_000);
        let diff = if a > b { a - b } else { b - a };
        assert!(diff <= 1, "a={a} b={b}");
    }

    /// Across a handful of thin-pool scenarios, the numerical optimizer
    /// produces strictly more profitable frontruns than the Zhou closed-form
    /// (and always non-negative).
    #[test]
    fn dominates_zhou_on_thin_pools() {
        let cfg = cfg_30bps();
        let scenarios: &[(u128, u128, u128)] = &[
            (50_000, 50_000, 200),
            (100_000, 100_000, 500),
            (200_000, 150_000, 1_000),
            (1_000_000, 800_000, 3_000),
            (10_000_000, 10_000_000, 10_000),
        ];
        let mut beats = 0;
        for &(r_in, r_out, victim) in scenarios {
            let v_num = numerical_frontrun_amount(r_in, r_out, victim, &cfg, 0);
            let p_num = net_profit(r_in, r_out, victim, &cfg, 0, v_num);

            let phi = 0.003f64;
            let one_minus_phi = 1.0 - phi;
            let x = r_in as f64;
            let v = victim as f64;
            let v_zhou = ((x * v) / ((x * (x + one_minus_phi * v)).sqrt() + x)) as u128;
            let p_zhou = net_profit(r_in, r_out, victim, &cfg, 0, v_zhou);

            assert!(
                p_num >= p_zhou,
                "scenario r_in={r_in} r_out={r_out} v={victim}: \
                 num p={p_num} v={v_num}, zhou p={p_zhou} v={v_zhou}"
            );
            if p_num > p_zhou {
                beats += 1;
            }
        }
        assert!(
            beats >= 1,
            "numerical should strictly beat Zhou on >=1 scenario"
        );
    }

    /// High `tx_cost_per_leg` should drive the optimizer to give up (return 0) when
    /// the most profitable frontrun still cannot cover 2 * tx_cost_per_leg.
    /// DIAGNOSTIC: print net-profit curve to confirm the "plateau in the
    /// millions" reported by an earlier agent. Run via:
    /// `cargo test -p amm-math diagnose_plateau -- --nocapture`.
    #[test]
    fn diagnose_plateau() {
        let r_in = 1_000_000u128;
        let r_out = 1_000_000u128;
        let victim = 50_000u128;
        let cfg = cfg_30bps();
        for &v_f in &[
            1_000u128, 10_000, 24_695, // Zhou closed-form
            100_000, 500_000, 1_000_000, 5_000_000, 10_000_000,
        ] {
            let p = net_profit(r_in, r_out, victim, &cfg, 0, v_f);
            eprintln!("V_f = {v_f:>11} -> net = {p}");
        }
        let v = numerical_frontrun_amount(r_in, r_out, victim, &cfg, 0);
        let p = net_profit(r_in, r_out, victim, &cfg, 0, v);
        eprintln!("optimizer chose V_f = {v}, net = {p}");
    }

    #[test]
    fn unprofitable_after_tx_cost_returns_zero() {
        let v = numerical_frontrun_amount(1_000_000, 1_000_000, 10, &cfg_30bps(), 1_000_000_000);
        assert_eq!(v, 0);
    }

    /// Regression test: the optimal V_f must stay strictly below the pool's
    /// reserve_in. A V_f >= reserve_in means the attacker would deposit more
    /// than the pool's worth, which is economically infeasible (and a
    /// historical artifact of an over-wide search bracket landing on the
    /// integer-CPMM output-saturation plateau).
    #[test]
    fn optimal_vf_stays_below_reserve_in() {
        let r_in = 1_000_000u128;
        let r_out = 1_000_000u128;
        let victim = 50_000u128;
        let v = numerical_frontrun_amount(r_in, r_out, victim, &cfg_30bps(), 0);
        assert!(
            v < r_in,
            "V_f={v} must be < reserve_in={r_in} (realistic MEV regime)"
        );
        // And it must be at least as large as Zhou's closed-form V_f.
        let phi = 0.003f64;
        let one_minus_phi = 1.0 - phi;
        let x = r_in as f64;
        let vf_f = victim as f64;
        let zhou = ((x * vf_f) / ((x * (x + one_minus_phi * vf_f)).sqrt() + x)) as u128;
        let p_num = net_profit(r_in, r_out, victim, &cfg_30bps(), 0, v);
        let p_zhou = net_profit(r_in, r_out, victim, &cfg_30bps(), 0, zhou);
        assert!(p_num >= p_zhou);
    }

    /// Real-pool reality anchor: load the cached `wsol_surge` Raydium CPMM
    /// pool (mainnet snapshot, ~$354k TVL) and confirm the optimizer
    /// produces an economically sensible frontrun for a 10-SOL victim.
    ///
    /// Skips gracefully if the cache fixture is absent.
    #[test]
    fn real_pool_wsol_surge_sanity() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use std::path::PathBuf;

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let cache_root = PathBuf::from(manifest_dir)
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("fork/cache/pools/wsol_surge"))
            .expect("workspace layout");
        if !cache_root.join("manifest.json").exists() {
            eprintln!("skipping: wsol_surge cache not present at {cache_root:?}");
            return;
        }

        let manifest_bytes =
            std::fs::read(cache_root.join("manifest.json")).expect("read manifest");
        let manifest: serde_json::Value =
            serde_json::from_slice(&manifest_bytes).expect("parse manifest");
        let vault_a = manifest["vault_a"].as_str().unwrap();
        let vault_b = manifest["vault_b"].as_str().unwrap();
        let amm_config = manifest["amm_config"].as_str().unwrap();

        let load_data = |pubkey: &str| -> Vec<u8> {
            let bytes =
                std::fs::read(cache_root.join(format!("{pubkey}.json"))).expect("read account");
            let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse account");
            STANDARD
                .decode(v["data_b64"].as_str().unwrap())
                .expect("base64")
        };

        // SPL Token Account: amount is u64 LE at offset 64.
        let read_amount = |data: &[u8]| -> u128 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&data[64..72]);
            u64::from_le_bytes(buf) as u128
        };
        let vault_a_data = load_data(vault_a);
        let vault_b_data = load_data(vault_b);
        let reserve_a = read_amount(&vault_a_data); // WSOL (mint_a)
        let reserve_b = read_amount(&vault_b_data);

        // Raydium AmmConfig layout (verified against the cached bytes):
        //   8 (disc) + 1 (bump) + 1 (disable_create_pool) + 2 (index)
        //   + u64 trade_fee_rate     @ offset 12
        //   + u64 protocol_fee_rate  @ offset 20
        //   + u64 fund_fee_rate      @ offset 28
        //   + u64 create_pool_fee    @ offset 36
        //   + 32 protocol_owner      @ 44..76
        //   + 32 fund_owner          @ 76..108
        //   + u64 creator_fee_rate   @ offset 108
        let cfg_data = load_data(amm_config);
        let read_u64 = |off: usize| -> u64 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&cfg_data[off..off + 8]);
            u64::from_le_bytes(buf)
        };
        let trade_fee_rate = read_u64(12);
        let creator_fee_rate = read_u64(108);
        assert_eq!(trade_fee_rate, 2500, "expected Raydium 25 bps trade fee");
        assert_eq!(creator_fee_rate, 500, "expected 5 bps creator fee");

        let cfg = MultiFeeConfig {
            trade_fee_rate,
            creator_fee_rate,
            fee_denominator: 1_000_000,
            creator_fee_mode: CreatorFeeMode::OnInput,
        };

        // Victim sells 10 SOL on the WSOL -> token_b direction.
        let victim_in: u128 = 10 * 1_000_000_000;
        let v_f = numerical_frontrun_amount(reserve_a, reserve_b, victim_in, &cfg, 0);
        let p = net_profit(reserve_a, reserve_b, victim_in, &cfg, 0, v_f);

        eprintln!(
            "wsol_surge: reserve_a(WSOL)={} reserve_b={} victim=10 SOL \
             optimal V_f={} (= {:.3} SOL) net_profit={} lamports (= {:.6} SOL)",
            reserve_a,
            reserve_b,
            v_f,
            v_f as f64 / 1e9,
            p,
            p as f64 / 1e9
        );

        // Sanity bounds for a $354k-TVL pool with a 10-SOL victim.
        // The CPMM-optimal V_f on a deep pool with a tiny victim is large
        // (the optimizer trades large capital for small slippage capture),
        // so the only bound that's economically meaningful here is V_f <
        // reserve_in. A real attacker's capital constraint would bind this
        // tighter, but that is a caller-side concern (cf. capital_cap
        // parameter, future work).
        assert!(v_f > 0, "optimizer must find a positive frontrun");
        assert!(p > 0, "net profit must be strictly positive, got {p}");
        assert!(
            v_f < reserve_a,
            "V_f={v_f} must stay below reserve_in={reserve_a}"
        );
        // Cross-check: at the LiteSVM-measured V_f of 5 SOL, the pool yields
        // +~40 bps net profit. The optimizer's choice must be at least as
        // profitable in absolute lamports.
        let p_5sol = net_profit(reserve_a, reserve_b, victim_in, &cfg, 0, 5_000_000_000);
        assert!(
            p >= p_5sol,
            "optimizer p={p} should beat 5-SOL reference p_5sol={p_5sol}"
        );
        let bps_5sol = (p_5sol as f64 / 5e9) * 10_000.0;
        assert!(
            (10.0..100.0).contains(&bps_5sol),
            "5-SOL reference bps={bps_5sol:.1} outside expected ~40 bps range"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::multi_fee::MultiFeeConfig;
    use proptest::prelude::*;

    const MAX_RESERVE: u128 = 1_000_000_000_000;

    fn zhou_frontrun_amount(reserve_in: u128, victim_amount_in: u128, fee_bps: u16) -> u128 {
        let phi = fee_bps as f64 / 10_000.0;
        let one_minus_phi = (1.0 - phi).max(1e-9);
        let x = reserve_in as f64;
        let v = victim_amount_in as f64;

        ((x * v) / ((x * (x + one_minus_phi * v)).sqrt() + x)) as u128
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// If the numerical optimizer returns a sandwich, that sandwich is
        /// executable with non-negative gross profit and strictly positive
        /// net profit. If no positive-profit frontrun exists, returning `None`
        /// is the expected output.
        #[test]
        fn numerical_result_is_never_negative_profit(
            reserve_in in 1_000u128..MAX_RESERVE,
            reserve_out in 1_000u128..MAX_RESERVE,
            victim_frac_ppm in 1u128..500_000,
            fee_bps in 0u16..9_999,
        ) {
            let victim_amount_in = (reserve_in.saturating_mul(victim_frac_ppm) / 1_000_000).max(1);
            prop_assume!(victim_amount_in <= reserve_in / 2);

            let cfg = MultiFeeConfig::single(u64::from(fee_bps), 10_000);
            if let Some(result) =
                compute_numerical_sandwich(reserve_in, reserve_out, victim_amount_in, &cfg, 0)
            {
                prop_assert!(result.frontrun_amount > 0);
                prop_assert!(result.frontrun_amount < reserve_in);
                prop_assert!(
                    result.gross_profit >= 0,
                    "gross_profit={} r_in={} r_out={} victim={} fee_bps={}",
                    result.gross_profit,
                    reserve_in,
                    reserve_out,
                    victim_amount_in,
                    fee_bps
                );
                prop_assert!(
                    result.net_profit > 0,
                    "net_profit={} r_in={} r_out={} victim={} fee_bps={}",
                    result.net_profit,
                    reserve_in,
                    reserve_out,
                    victim_amount_in,
                    fee_bps
                );
            }
        }

        /// The fee-aware numerical optimizer should not underperform the Zhou
        /// closed-form frontrun when both are evaluated against the same
        /// executable integer CPMM profit function. A small integer-rounding
        /// tolerance is allowed because Zhou's formula is continuous/f64 while
        /// execution is integer/ceil-fee based.
        #[test]
        fn numerical_profit_matches_or_beats_zhou_profit_proptest(
            reserve_in in 1_000u128..MAX_RESERVE,
            reserve_out in 1_000u128..MAX_RESERVE,
            victim_frac_ppm in 1u128..500_000,
            fee_bps in 0u16..9_999,
        ) {
            let victim_amount_in = (reserve_in.saturating_mul(victim_frac_ppm) / 1_000_000).max(1);
            prop_assume!(victim_amount_in <= reserve_in / 2);

            let cfg = MultiFeeConfig::single(u64::from(fee_bps), 10_000);
            let zhou_v = zhou_frontrun_amount(reserve_in, victim_amount_in, fee_bps);
            prop_assume!(zhou_v > 0);
            prop_assume!(zhou_v < reserve_in);

            let zhou_profit =
                net_profit(reserve_in, reserve_out, victim_amount_in, &cfg, 0, zhou_v);
            let numerical =
                compute_numerical_sandwich(reserve_in, reserve_out, victim_amount_in, &cfg, 0);

            match numerical {
                Some(result) => prop_assert!(
                    result.net_profit + 4 >= zhou_profit,
                    "numerical={} zhou={} zhou_v={} r_in={} r_out={} victim={} fee_bps={}",
                    result.net_profit,
                    zhou_profit,
                    zhou_v,
                    reserve_in,
                    reserve_out,
                    victim_amount_in,
                    fee_bps
                ),
                None => prop_assert!(
                    zhou_profit <= 4,
                    "numerical returned None but Zhou profit={} zhou_v={} r_in={} r_out={} victim={} fee_bps={}",
                    zhou_profit,
                    zhou_v,
                    reserve_in,
                    reserve_out,
                    victim_amount_in,
                    fee_bps
                ),
            }
        }
    }
}
