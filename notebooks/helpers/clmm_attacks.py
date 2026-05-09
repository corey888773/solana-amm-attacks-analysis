"""Helpers for analyzing CLMM live attack evaluation output.

Reads `historical_clmm_candidates.csv` produced by
`fork::historical_clmm::attack::evaluate_live_attacks` and surfaces
best-attempt diagnostics introduced alongside the profitability filter
removal.

Schema reference: `fork/src/historical_clmm/attack.rs::LiveClmmAttackRow`.
"""

from __future__ import annotations

import numpy as np
import pandas as pd


REJECTION_LABELS = {
    "no_grid_result": "no grid result (zero steps / replay failed before grid)",
    "best_attempt_infeasible": "best attempt breached victim slippage tolerance",
    "best_attempt_unprofitable": "best attempt feasible but net profit <= 0",
    "no_profitable_attack": "legacy: no profitable attack (pre-best_attempt schema)",
}


def attach_diagnostics(df: pd.DataFrame) -> pd.DataFrame:
    """Add derived columns used by downstream summaries.

    - `evaluated`: model_status == "evaluated" (i.e. grid actually ran)
    - `fee_drag`: 2 * tx_cost_per_leg (round-trip transaction cost)
    - `best_attempt_gross_minus_drag`: gross profit minus fee drag
    """
    out = df.copy()
    if "model_status" in out:
        out["evaluated"] = out["model_status"].astype(str).eq("evaluated")
    else:
        out["evaluated"] = False
    if "tx_cost_per_leg" in out:
        out["fee_drag"] = pd.to_numeric(out["tx_cost_per_leg"], errors="coerce") * 2
    else:
        out["fee_drag"] = np.nan
    if "best_attempt_gross_profit" in out:
        gross = pd.to_numeric(out["best_attempt_gross_profit"], errors="coerce")
        out["best_attempt_gross_minus_drag"] = gross - out["fee_drag"]
    return out


def rejection_breakdown(df: pd.DataFrame) -> pd.DataFrame:
    """Counts of rejection_reason among evaluated rows, with labels."""
    if "rejection_reason" not in df:
        return pd.DataFrame(columns=["rejection_reason", "rows", "label"])
    reasons = (
        df["rejection_reason"]
        .fillna("evaluated_profitable")
        .astype(str)
        .value_counts()
        .rename_axis("rejection_reason")
        .reset_index(name="rows")
    )
    reasons["label"] = reasons["rejection_reason"].map(
        lambda r: REJECTION_LABELS.get(r, r),
    )
    return reasons


def best_attempt_summary(df: pd.DataFrame) -> pd.DataFrame:
    """Summary stats for evaluated rows partitioned by feasibility/profit."""
    if "model_status" not in df:
        return pd.DataFrame()
    evaluated = df[df["model_status"].astype(str).eq("evaluated")].copy()
    if evaluated.empty:
        return pd.DataFrame()

    evaluated["bucket"] = np.select(
        [
            evaluated.get("attack_profitable", False).astype(bool),
            ~evaluated["best_attempt_feasible"].astype(bool),
        ],
        ["profitable", "infeasible"],
        default="unprofitable_feasible",
    )

    cols = {
        "best_attempt_net_profit": "median_best_net_profit",
        "best_attempt_gross_profit": "median_best_gross_profit",
        "best_attempt_victim_loss_absolute": "median_victim_loss",
        "best_attempt_victim_extra_slippage_bps": "median_victim_slippage_bps",
        "best_attempt_frontrun": "median_best_frontrun",
        "amount_in": "median_victim_amount_in",
    }
    agg = (
        evaluated.groupby("bucket")[list(cols.keys())]
        .median(numeric_only=True)
        .rename(columns=cols)
    )
    counts = evaluated.groupby("bucket").size().rename("rows")
    return counts.to_frame().join(agg).reset_index()


def replay_validation_summary(df: pd.DataFrame) -> dict:
    """Replay error stats over rows where the model actually ran.

    `replay_error_bps` is `|fair_amount_out - actual_amount_out| /
    actual_amount_out * 10000`. Computed for every row with usable
    pre-state, regardless of model_status. The pipeline rejects rows
    above `--replay-tolerance-bps` (default 100) as
    `victim_replay_mismatch`.
    """
    if "replay_error_bps" not in df:
        return {}
    err = pd.to_numeric(df["replay_error_bps"], errors="coerce").dropna()
    if err.empty:
        return {}
    evaluated_mask = df["model_status"].astype(str).eq("evaluated")
    evaluated_err = pd.to_numeric(
        df.loc[evaluated_mask, "replay_error_bps"], errors="coerce",
    ).dropna()
    return {
        "n_with_replay": int(len(err)),
        "n_evaluated": int(len(evaluated_err)),
        "median_bps": float(evaluated_err.median()) if len(evaluated_err) else None,
        "mean_bps": float(evaluated_err.mean()) if len(evaluated_err) else None,
        "p95_bps": float(evaluated_err.quantile(0.95)) if len(evaluated_err) else None,
        "max_bps": float(evaluated_err.max()) if len(evaluated_err) else None,
        "outliers_rejected": int(
            (df.get("rejection_reason", "") == "victim_replay_mismatch").sum()
        ),
    }


def fee_drag_vs_gross(df: pd.DataFrame) -> pd.Series:
    """Distribution of (best_attempt_gross_profit - fee_drag).

    Negative values mean even ignoring slippage feasibility, the attempt could
    not cover round-trip transaction cost.
    """
    diag = attach_diagnostics(df)
    return diag.loc[diag["evaluated"], "best_attempt_gross_minus_drag"].dropna()
