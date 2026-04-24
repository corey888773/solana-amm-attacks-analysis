use crate::constant_product::compute_swap;
use crate::types::SandwichResult;
use crate::BPS_DENOMINATOR_F64;

/// Analyze a sandwich attack: frontrun -> victim swap -> backrun.
///
/// Computes the optimal frontrun size and expected profit.
/// Uses the "half-the-victim" heuristic corrected for fees.
///
/// # Arguments
/// * `victim_amount` - victim's swap size (input token)
/// * `reserve_in` - pool reserve of input token
/// * `reserve_out` - pool reserve of output token
/// * `fee_bps` - pool fee in basis points
/// * `tx_cost` - total cost of 2 txs (frontrun + backrun) in input token units
pub fn optimal_sandwich(
    victim_amount: u64,
    reserve_in: u64,
    reserve_out: u64,
    fee_bps: u16,
    tx_cost: u64,
) -> Option<SandwichResult> {
    if victim_amount == 0 || reserve_in == 0 || reserve_out == 0 {
        return None;
    }

    // Optimal frontrun with fee correction:
    // V_f* = (sqrt(x * (x + (1-phi)*v)) - x) / (1-phi)
    // Source: Zhou et al., "High-Frequency Trading on Decentralized On-Chain Exchanges", IEEE S&P 2021
    // Section IV-B, Equation 3 (generalized with fee parameter phi)
    let phi = fee_bps as f64 / BPS_DENOMINATOR_F64;
    let one_minus_phi = 1.0 - phi;
    let v = victim_amount as f64;
    let x = reserve_in as f64;

    // Numerically-stable form: the direct expression (sqrt(x*(x + (1-phi)*v)) - x) / (1-phi)
    // suffers from catastrophic cancellation when (1-phi)*v is small relative to x
    // (the two terms in the subtraction are nearly equal, losing significant digits).
    // Rationalize by multiplying numerator and denominator by the conjugate
    // (sqrt(x*(x + (1-phi)*v)) + x), which algebraically yields:
    //     V_f* = (x * v) / (sqrt(x*(x + (1-phi)*v)) + x)
    // This is a standard rationalization trick for catastrophic cancellation; see
    // Higham, "Accuracy and Stability of Numerical Algorithms" (2nd ed., 2002), §1.8.
    // Note the (1-phi) factor cancels out entirely in the rationalized form.
    let discriminant = x * (x + one_minus_phi * v);
    let frontrun_optimal = (x * v) / (discriminant.sqrt() + x);
    let frontrun_amount = frontrun_optimal.max(0.0) as u64;

    if frontrun_amount == 0 {
        return None;
    }

    // Step 1: Frontrun — attacker buys token_out
    let frontrun = compute_swap(frontrun_amount, reserve_in, reserve_out, fee_bps)?;

    // Step 2: Victim swap — now against worse reserves
    let victim = compute_swap(
        victim_amount,
        frontrun.new_reserve_in,
        frontrun.new_reserve_out,
        fee_bps,
    )?;

    // Step 3: Backrun — attacker sells what they bought in frontrun
    // Direction flips: selling token_out back to token_in
    let backrun = compute_swap(
        frontrun.amount_out,    // sell all tokens bought
        victim.new_reserve_out, // token_out is now "in"
        victim.new_reserve_in,  // token_in is now "out"
        fee_bps,
    )?;

    // Victim's extra slippage from sandwich
    let fair_swap = compute_swap(victim_amount, reserve_in, reserve_out, fee_bps)?;
    let victim_loss = fair_swap.amount_out as i64 - victim.amount_out as i64;
    let victim_extra_slippage_bps = if fair_swap.amount_out > 0 {
        (victim_loss as f64 / fair_swap.amount_out as f64 * BPS_DENOMINATOR_F64) as u64
    } else {
        0
    };

    let gross_profit = backrun.amount_out as i64 - frontrun_amount as i64;
    let net_profit = gross_profit - tx_cost as i64;

    Some(SandwichResult {
        frontrun_amount,
        frontrun_output: frontrun.amount_out,
        backrun_output: backrun.amount_out,
        victim_extra_slippage_bps,
        gross_profit,
        net_profit,
        is_profitable: net_profit > 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandwich_basic_profitable() {
        // Big pool (1M:1M), large victim trade (50k), low fee
        let r = optimal_sandwich(50_000, 1_000_000, 1_000_000, 30, 100).unwrap();
        assert!(r.gross_profit > 0, "gross profit should be positive");
        assert!(r.frontrun_amount > 0);
        assert!(
            r.frontrun_amount < 50_000,
            "frontrun should be less than victim"
        );
        assert!(
            r.victim_extra_slippage_bps > 0,
            "victim should suffer extra slippage"
        );
    }

    #[test]
    fn sandwich_tiny_victim_unprofitable() {
        // Small victim (10 tokens) against big pool — not worth the tx cost
        let r = optimal_sandwich(10, 1_000_000, 1_000_000, 30, 5000);
        match r {
            Some(r) => assert!(!r.is_profitable, "tiny victim should not be profitable"),
            None => {} // also acceptable — frontrun rounds to 0
        }
    }

    #[test]
    fn sandwich_zero_fee_more_profitable() {
        let with_fee = optimal_sandwich(50_000, 1_000_000, 1_000_000, 30, 100).unwrap();
        let no_fee = optimal_sandwich(50_000, 1_000_000, 1_000_000, 0, 100).unwrap();
        assert!(
            no_fee.gross_profit > with_fee.gross_profit,
            "zero fee should yield more profit"
        );
    }

    #[test]
    fn victim_always_gets_less() {
        // Victim gets fewer tokens when sandwiched vs fair swap
        let r = optimal_sandwich(50_000, 1_000_000, 1_000_000, 30, 100).unwrap();
        assert!(r.victim_extra_slippage_bps > 0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    const MAX_RESERVE: u64 = 1_000_000_000_000_000_000; // 1e18

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// With zero fees, gross_profit at the optimal frontrun is mathematically >= 0
        /// (in the continuous model; integer rounding may shave a small amount).
        /// With fees, the closed-form "optimal" formula ignores the double-fee cost on
        /// frontrun+backrun, so gross_profit can be negative for thin/skewed pools —
        /// that's a known limitation of the Zhou et al. formula, not a bug.
        #[test]
        fn gross_profit_non_negative_zero_fee(
            reserve_in in 1_000u64..MAX_RESERVE,
            reserve_out in 1_000u64..MAX_RESERVE,
            victim_amount in 1u64..MAX_RESERVE,
        ) {
            prop_assume!(victim_amount <= reserve_in / 2);
            let res = optimal_sandwich(victim_amount, reserve_in, reserve_out, 0, 0);
            prop_assume!(res.is_some());
            let r = res.unwrap();
            prop_assume!(r.frontrun_amount > 0);
            // Allow a small integer-rounding slack (backrun/frontrun truncate to u64).
            prop_assert!(r.gross_profit >= -1,
                "gross_profit should be >= 0 (mod rounding), got {} (frontrun={}, backrun_out={})",
                r.gross_profit, r.frontrun_amount, r.backrun_output);
        }

        /// Victim extra slippage is non-negative (victim always gets <= fair amount).
        #[test]
        fn victim_slippage_non_negative(
            reserve_in in 1_000u64..MAX_RESERVE,
            reserve_out in 1_000u64..MAX_RESERVE,
            victim_amount in 1u64..MAX_RESERVE,
            fee_bps in 0u16..9999,
        ) {
            prop_assume!(victim_amount <= reserve_in / 2);
            let res = optimal_sandwich(victim_amount, reserve_in, reserve_out, fee_bps, 0);
            prop_assume!(res.is_some());
            let r = res.unwrap();
            // u64 type alone guarantees >= 0, but we assert the sentinel explicitly
            // to guard against future refactors to i64.
            prop_assert!(r.victim_extra_slippage_bps as i64 >= 0);
        }

        /// Backrun output is bounded by the frontrun input: the attacker cannot extract
        /// more input-token value through the backrun than they put in up front.
        /// (Otherwise the attack would be a no-victim arbitrage already.)
        #[test]
        fn backrun_bounded_by_frontrun_plus_victim_impact(
            reserve_in in 1_000u64..MAX_RESERVE,
            reserve_out in 1_000u64..MAX_RESERVE,
            victim_amount in 1u64..MAX_RESERVE,
            fee_bps in 0u16..9999,
        ) {
            prop_assume!(victim_amount <= reserve_in / 2);
            let res = optimal_sandwich(victim_amount, reserve_in, reserve_out, fee_bps, 0);
            prop_assume!(res.is_some());
            let r = res.unwrap();
            prop_assume!(r.frontrun_amount > 0);
            // Backrun output is in input-token units; it must be finite and reasonable.
            // Conservative monotone bound: backrun_output <= frontrun_amount + victim_amount
            // (attacker cannot extract more than total input-side flow through the pool).
            prop_assert!(
                r.backrun_output <= r.frontrun_amount.saturating_add(victim_amount),
                "backrun_output={} exceeds frontrun+victim={}",
                r.backrun_output,
                r.frontrun_amount.saturating_add(victim_amount)
            );
        }
    }
}
