use crate::constant_product::compute_swap;
use crate::types::SandwichResult;

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
    let phi = fee_bps as f64 / 10_000.0;
    let one_minus_phi = 1.0 - phi;
    let v = victim_amount as f64;
    let x = reserve_in as f64;

    let frontrun_optimal = ((x * (x + one_minus_phi * v)).sqrt() - x) / one_minus_phi;
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
        frontrun.amount_out,      // sell all tokens bought
        victim.new_reserve_out,   // token_out is now "in"
        victim.new_reserve_in,    // token_in is now "out"
        fee_bps,
    )?;

    // Victim's extra slippage from sandwich
    let fair_swap = compute_swap(victim_amount, reserve_in, reserve_out, fee_bps)?;
    let victim_loss = fair_swap.amount_out as i64 - victim.amount_out as i64;
    let victim_extra_slippage_bps = if fair_swap.amount_out > 0 {
        (victim_loss as f64 / fair_swap.amount_out as f64 * 10_000.0) as u64
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
        assert!(r.frontrun_amount < 50_000, "frontrun should be less than victim");
        assert!(r.victim_extra_slippage_bps > 0, "victim should suffer extra slippage");
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
