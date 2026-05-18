#!/usr/bin/env python3
"""Augment a candidates CSV with two post-hoc columns:

    victim_slippage_tolerance_bps  derived from (fair_amount_out, min_amount_out):
                                   how much extra slippage (in bps) the victim's
                                   on-chain `min_amount_out` actually allows.
                                   0 means strict (no slippage tolerated past fair),
                                   10_000 means up to 100% slippage envelope.

    actually_feasible              true iff `victim_amount_after_sandwich >=
                                   min_amount_out`, i.e. the optimizer's chosen
                                   frontrun does not push the victim below their
                                   on-chain protection (no revert).

The optimizer's existing `attack_feasible` flag is a misnomer — it is hardcoded
true whenever the optimizer found a positive-profit frontrun, without checking
the victim's `min_amount_out`. This script computes the real feasibility
post-hoc.

Usage:
    scripts/add_real_feasibility.py <candidates_csv> [output_csv]

If output_csv is omitted, writes <input>_with_real_feas.csv next to input.
"""

import csv
import sys
from pathlib import Path


def slippage_tolerance_bps(fair: int, min_out: int) -> int:
    """Victim's on-chain slippage envelope, in bps relative to fair output."""
    if fair <= 0:
        return 0
    if min_out >= fair:
        return 0
    return (fair - min_out) * 10_000 // fair


def actually_feasible(fair: int, min_out: int, extra_slip_bps: int) -> bool:
    """Would the victim's tx land after the sandwich?
    victim_after = fair * (10_000 - extra_slip_bps) / 10_000
    feasible iff victim_after >= min_out."""
    if fair <= 0:
        return False
    eff = max(0, 10_000 - extra_slip_bps)
    victim_after = fair * eff // 10_000
    return victim_after >= min_out


def parse_int(v):
    if v in (None, "", "None"):
        return None
    try:
        return int(float(v))
    except (TypeError, ValueError):
        return None


def augment(input_csv: Path, output_csv: Path) -> dict:
    rows_in = list(csv.DictReader(open(input_csv)))
    if not rows_in:
        raise SystemExit(f"no rows in {input_csv}")

    fieldnames = list(rows_in[0].keys())
    for new_col in ("victim_slippage_tolerance_bps", "actually_feasible"):
        if new_col not in fieldnames:
            fieldnames.append(new_col)

    stats = {
        "rows": 0,
        "evaluated": 0,
        "math_profitable": 0,
        "actually_feasible": 0,
        "math_profitable_AND_actually_feasible": 0,
    }

    with open(output_csv, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fieldnames)
        w.writeheader()
        for row in rows_in:
            stats["rows"] += 1
            fair = parse_int(row.get("fair_amount_out"))
            mn = parse_int(row.get("min_amount_out"))
            extra = parse_int(row.get("victim_extra_slippage_bps")) or 0
            tol = None
            feas = None
            if fair is not None and mn is not None:
                tol = slippage_tolerance_bps(fair, mn)
                feas = actually_feasible(fair, mn, extra)
            row["victim_slippage_tolerance_bps"] = "" if tol is None else tol
            row["actually_feasible"] = "" if feas is None else ("true" if feas else "false")

            if row.get("model_status") == "evaluated":
                stats["evaluated"] += 1
                if row.get("attack_profitable") == "true":
                    stats["math_profitable"] += 1
                    if feas:
                        stats["math_profitable_AND_actually_feasible"] += 1
                if feas:
                    stats["actually_feasible"] += 1

            w.writerow(row)
    return stats


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    inp = Path(sys.argv[1])
    out = Path(sys.argv[2]) if len(sys.argv) >= 3 else inp.with_name(inp.stem + "_with_real_feas.csv")
    print(f"input  : {inp}")
    print(f"output : {out}")
    s = augment(inp, out)
    print()
    print("summary:")
    for k, v in s.items():
        print(f"  {k:45s} {v}")


if __name__ == "__main__":
    main()
