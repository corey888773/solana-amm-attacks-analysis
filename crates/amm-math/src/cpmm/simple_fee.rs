use crate::types::SwapResult;
use crate::{BPS_DENOMINATOR, BPS_DENOMINATOR_F64};

/// Compute output of a constant-product swap (x * y = k).
///
/// Fee is deducted from input before applying the invariant.
/// All arithmetic uses u128 intermediates to prevent overflow.
///
/// # Arguments
/// * `amount_in` - tokens being swapped in
/// * `reserve_in` - current reserve of input token
/// * `reserve_out` - current reserve of output token
/// * `fee_bps` - fee in basis points (e.g. 30 = 0.30%)
///
/// # Returns
/// `None` if inputs are zero or fee >= 100%
pub fn compute_swap(
    amount_in: u128,
    reserve_in: u128,
    reserve_out: u128,
    fee_bps: u16,
) -> Option<SwapResult> {
    if amount_in == 0 || reserve_in == 0 || reserve_out == 0 || fee_bps as u128 >= BPS_DENOMINATOR {
        return None;
    }

    // Deduct fee from input
    let amount_after_fee =
        amount_in.checked_mul(BPS_DENOMINATOR - fee_bps as u128)? / BPS_DENOMINATOR;

    // Constant product formula: dy = (y * dx_after_fee) / (x + dx_after_fee)
    // Source: Uniswap V2 whitepaper (Adams et al., 2020), Section 3.1.1
    let numerator = amount_after_fee.checked_mul(reserve_out)?;
    let denominator = reserve_in.checked_add(amount_after_fee)?;
    let amount_out = numerator / denominator;

    if amount_out == 0 || amount_out >= reserve_out {
        return None;
    }

    let fee_amount = amount_in.checked_sub(amount_after_fee)?;
    // `reserve_in + amount_in` can overflow for near-u128::MAX pools; guard defensively.
    let new_reserve_in = reserve_in.checked_add(amount_in)?;
    let new_reserve_out = reserve_out.checked_sub(amount_out)?;

    let price_before = reserve_out as f64 / reserve_in as f64;
    let price_after = new_reserve_out as f64 / new_reserve_in as f64;
    let price_impact_bps = ((1.0 - price_after / price_before) * BPS_DENOMINATOR_F64) as u64;

    Some(SwapResult {
        amount_out,
        fee_amount,
        trade_fee: fee_amount,
        creator_fee: 0,
        price_before,
        price_after,
        price_impact_bps,
        new_reserve_in,
        new_reserve_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_swap_no_fee() {
        // Pool: 1000 A, 1000 B, 0% fee
        // Swap 100 A -> should get ~90.9 B (constant product)
        let r = compute_swap(100, 1000, 1000, 0).unwrap();
        // dy = 1000 * 100 / (1000 + 100) = 90.909... -> 90 (truncated)
        assert_eq!(r.amount_out, 90);
        assert_eq!(r.fee_amount, 0);
        assert_eq!(r.new_reserve_in, 1100);
        assert_eq!(r.new_reserve_out, 910);
        // k should be preserved or increase (from rounding in protocol's favor)
        assert!(r.new_reserve_in as u128 * r.new_reserve_out as u128 >= 1000u128 * 1000);
    }

    #[test]
    fn swap_with_30bps_fee() {
        // Pool: 1_000_000 A, 1_000_000 B, 0.30% fee (typical Uniswap/Raydium)
        let r = compute_swap(10_000, 1_000_000, 1_000_000, 30).unwrap();
        // amount_after_fee = 10_000 * 9970 / 10000 = 9970
        // dy = 1_000_000 * 9970 / (1_000_000 + 9970) = 9871 (approx)
        assert_eq!(r.fee_amount, 30); // 10_000 * 30 / 10_000
        assert!(r.amount_out > 0);
        assert!(r.amount_out < 10_000); // always get less than input at 1:1 pool
    }

    #[test]
    fn swap_zero_inputs_return_none() {
        assert!(compute_swap(0, 1000, 1000, 30).is_none());
        assert!(compute_swap(100, 0, 1000, 30).is_none());
        assert!(compute_swap(100, 1000, 0, 30).is_none());
    }

    #[test]
    fn swap_fee_100pct_returns_none() {
        assert!(compute_swap(100, 1000, 1000, 10_000).is_none());
    }

    #[test]
    fn k_invariant_preserved() {
        // k should never decrease after a swap (rounding favors pool)
        let r = compute_swap(500, 10_000, 10_000, 30).unwrap();
        let k_before = 10_000u128 * 10_000;
        let k_after = r.new_reserve_in as u128 * r.new_reserve_out as u128;
        assert!(
            k_after >= k_before,
            "k must not decrease: {k_after} < {k_before}"
        );
    }

    #[test]
    fn overflow_returns_none() {
        // reserve_in + amount_in would overflow u128
        assert!(compute_swap(u128::MAX, u128::MAX, 1_000_000, 30).is_none());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    const MAX_RESERVE: u128 = 1_000_000_000_000_000_000; // 1e18

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Conservation of token_out: amount_out + new_reserve_out == reserve_out.
        #[test]
        fn conservation_of_token_out(
            reserve_in in 1_000u128..MAX_RESERVE,
            reserve_out in 1_000u128..MAX_RESERVE,
            amount_in_frac in 1u128..1_000_000,
            fee_bps in 0u16..9999,
        ) {
            let amount_in = (reserve_in / 2).max(1).min(amount_in_frac.saturating_mul(reserve_in / 1_000_000).max(1));
            let res = compute_swap(amount_in, reserve_in, reserve_out, fee_bps);
            prop_assume!(res.is_some());
            let r = res.unwrap();
            prop_assert_eq!(r.amount_out + r.new_reserve_out, reserve_out);
        }

        /// k-invariant non-decreasing (rounding favors the pool).
        #[test]
        fn k_invariant_non_decreasing(
            reserve_in in 1_000u128..MAX_RESERVE,
            reserve_out in 1_000u128..MAX_RESERVE,
            amount_in in 1u128..MAX_RESERVE,
            fee_bps in 0u16..9999,
        ) {
            prop_assume!(amount_in <= reserve_in / 2);
            let res = compute_swap(amount_in, reserve_in, reserve_out, fee_bps);
            prop_assume!(res.is_some());
            let r = res.unwrap();
            let k_before = reserve_in * reserve_out;
            let k_after = r.new_reserve_in * r.new_reserve_out;
            prop_assert!(k_after >= k_before, "k decreased: before={} after={}", k_before, k_after);
        }

        /// Round-trip (swap then reverse swap) loses value to fees: out <= amount_in.
        /// With zero fee it may equal amount_in minus rounding; with fee it is strictly less.
        #[test]
        fn round_trip_loses_to_fees(
            reserve_in in 1_000u128..MAX_RESERVE,
            reserve_out in 1_000u128..MAX_RESERVE,
            amount_in in 1u128..MAX_RESERVE,
            fee_bps in 1u16..9999,
        ) {
            prop_assume!(amount_in <= reserve_in / 2);
            let fwd = compute_swap(amount_in, reserve_in, reserve_out, fee_bps);
            prop_assume!(fwd.is_some());
            let fwd = fwd.unwrap();
            let back = compute_swap(fwd.amount_out, fwd.new_reserve_out, fwd.new_reserve_in, fee_bps);
            prop_assume!(back.is_some());
            let back = back.unwrap();
            prop_assert!(back.amount_out < amount_in,
                "round-trip should lose to fees: got {} from {}", back.amount_out, amount_in);
        }
    }
}
