use crate::types::SwapResult;

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
    amount_in: u64,
    reserve_in: u64,
    reserve_out: u64,
    fee_bps: u16,
) -> Option<SwapResult> {
    if amount_in == 0 || reserve_in == 0 || reserve_out == 0 || fee_bps >= 10_000 {
        return None;
    }

    let amount_in_128 = amount_in as u128;
    let reserve_in_128 = reserve_in as u128;
    let reserve_out_128 = reserve_out as u128;

    // Deduct fee from input
    let amount_after_fee = amount_in_128 * (10_000 - fee_bps as u128) / 10_000;

    // Constant product formula: dy = (y * dx_after_fee) / (x + dx_after_fee)
    let numerator = amount_after_fee * reserve_out_128;
    let denominator = reserve_in_128 + amount_after_fee;
    let amount_out = (numerator / denominator) as u64;

    if amount_out == 0 || amount_out >= reserve_out {
        return None;
    }

    let fee_amount = amount_in - (amount_after_fee as u64);
    let new_reserve_in = reserve_in + amount_in;
    let new_reserve_out = reserve_out - amount_out;

    let price_before = reserve_out as f64 / reserve_in as f64;
    let price_after = new_reserve_out as f64 / new_reserve_in as f64;
    let price_impact_bps = ((1.0 - price_after / price_before) * 10_000.0) as u64;

    Some(SwapResult {
        amount_out,
        fee_amount,
        price_before,
        price_after,
        price_impact_bps,
        new_reserve_in,
        new_reserve_out,
    })
}

/// Compute price impact in basis points for a given trade size.
pub fn price_impact_bps(reserve_in: u64, reserve_out: u64, amount_in: u64) -> f64 {
    let price_before = reserve_out as f64 / reserve_in as f64;
    let effective_price = reserve_out as f64 / (reserve_in as f64 + amount_in as f64);
    (1.0 - effective_price / price_before) * 10_000.0
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
        assert!(k_after >= k_before, "k must not decrease: {k_after} < {k_before}");
    }
}
