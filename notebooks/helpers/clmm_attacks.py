"""Helpers for analyzing CLMM live attack evaluation output.

Reads `historical_clmm_candidates.csv` produced by
`fork::historical_clmm::attack::evaluate_live_attacks` and surfaces
best-attempt diagnostics introduced alongside the profitability filter
removal.

Schema reference: `fork/src/historical_clmm/attack.rs::LiveClmmAttackRow`.
"""

from __future__ import annotations

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns


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


def _signed_log10(series):
    values = pd.to_numeric(series, errors="coerce")
    return np.sign(values) * np.log10(np.abs(values) + 1)


def plot_replay_error_hist(df: pd.DataFrame):
    """Histogram of replay_error_bps over evaluated rows, with p95 / max
    reference lines."""
    if df.empty or "replay_error_bps" not in df:
        return None
    err = pd.to_numeric(
        df.loc[df["model_status"].astype(str).eq("evaluated"), "replay_error_bps"],
        errors="coerce",
    ).dropna()
    if err.empty or err.nunique() < 2:
        return None

    fig, ax = plt.subplots(figsize=(9, 4.5))
    sns.histplot(err, bins=30, color="#4c78a8", ax=ax)
    p95 = float(err.quantile(0.95))
    mx = float(err.max())
    ax.axvline(p95, color="orange", linestyle="--", linewidth=1, label=f"p95 = {p95:.0f} bps")
    ax.axvline(mx, color="red", linestyle="--", linewidth=1, label=f"max = {mx:.0f} bps")
    ax.set_title("CLMM replay error: model fair_amount_out vs on-chain actual")
    ax.set_xlabel("replay_error_bps")
    ax.set_ylabel("evaluated swaps")
    ax.legend()
    fig.tight_layout()
    return fig


def plot_attempt_buckets_by_pool(df: pd.DataFrame):
    """Stacked bar of best-attempt outcome buckets per pool.

    Buckets: profitable / unprofitable_feasible / infeasible / rejected.
    """
    if df.empty or "pool_label" not in df:
        return None
    work = df.copy()
    profitable = work.get("attack_profitable", False)
    if profitable.dtype != bool:
        profitable = profitable.astype(str).str.lower().isin(["true", "1"])
    feasible = work.get("best_attempt_feasible", False)
    if feasible.dtype != bool:
        feasible = feasible.astype(str).str.lower().isin(["true", "1"])
    evaluated = work["model_status"].astype(str).eq("evaluated")
    work["bucket"] = np.select(
        [
            ~evaluated,
            profitable,
            ~feasible,
        ],
        ["rejected", "profitable", "infeasible"],
        default="unprofitable_feasible",
    )
    counts = (
        work.groupby(["pool_label", "bucket"])
        .size()
        .unstack(fill_value=0)
        .reindex(
            columns=["profitable", "unprofitable_feasible", "infeasible", "rejected"],
            fill_value=0,
        )
    )
    if counts.empty or counts.values.sum() == 0:
        return None

    fig, ax = plt.subplots(figsize=(9, 4.5))
    counts.plot(
        kind="barh",
        stacked=True,
        ax=ax,
        color=["#54a24b", "#4c78a8", "#e45756", "#bab0ac"],
    )
    ax.set_title("CLMM attack outcomes by pool")
    ax.set_xlabel("rows")
    ax.set_ylabel("")
    ax.legend(loc="lower right")
    fig.tight_layout()
    return fig


def plot_frontrun_vs_victim(df: pd.DataFrame):
    """Scatter of frontrun vs victim amount on log-log axes for profitable rows.

    Highlights the capital-efficiency caveat: many profitable rows require
    frontrun >> victim, which is impractical for retail MEV bots.
    """
    if df.empty:
        return None
    profitable = df.get("attack_profitable", False)
    if profitable.dtype != bool:
        profitable = profitable.astype(str).str.lower().isin(["true", "1"])
    work = df.loc[profitable].copy()
    if work.empty:
        return None
    work["victim_amount_in_num"] = pd.to_numeric(
        work["amount_in"], errors="coerce",
    )
    work["frontrun_num"] = pd.to_numeric(
        work["optimal_frontrun"], errors="coerce",
    )
    work = work.dropna(subset=["victim_amount_in_num", "frontrun_num"])
    work = work[(work["victim_amount_in_num"] > 0) & (work["frontrun_num"] > 0)]
    if work.empty:
        return None

    fig, ax = plt.subplots(figsize=(8, 5))
    sns.scatterplot(
        data=work,
        x="victim_amount_in_num",
        y="frontrun_num",
        hue="pool_label",
        ax=ax,
    )
    lo = min(work["victim_amount_in_num"].min(), work["frontrun_num"].min())
    hi = max(work["victim_amount_in_num"].max(), work["frontrun_num"].max())
    ax.plot([lo, hi], [lo, hi], color="black", linewidth=1, linestyle="--", label="frontrun = victim")
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_title("Profitable rows: frontrun size vs victim size (log-log)")
    ax.set_xlabel("victim amount_in [lamports]")
    ax.set_ylabel("optimal_frontrun [lamports]")
    ax.legend()
    fig.tight_layout()
    return fig


def plot_victim_slippage_breach(df: pd.DataFrame):
    """Histogram of victim_extra_slippage_bps for profitable rows.

    Reference lines at 100 / 1000 bps mark realistic vs degenerate
    slippage tolerance regimes.
    """
    if df.empty:
        return None
    profitable = df.get("attack_profitable", False)
    if profitable.dtype != bool:
        profitable = profitable.astype(str).str.lower().isin(["true", "1"])
    err = pd.to_numeric(
        df.loc[profitable, "victim_extra_slippage_bps"], errors="coerce",
    ).dropna()
    if err.empty or err.nunique() < 2:
        return None

    fig, ax = plt.subplots(figsize=(9, 4.5))
    sns.histplot(err, bins=30, color="#e45756", ax=ax)
    ax.axvline(100, color="black", linewidth=1, linestyle="--", label="100 bps")
    ax.axvline(1000, color="orange", linewidth=1, linestyle="--", label="1 000 bps")
    ax.set_title("Profitable rows: victim slippage breach distribution")
    ax.set_xlabel("victim_extra_slippage_bps")
    ax.set_ylabel("profitable rows")
    ax.legend()
    fig.tight_layout()
    return fig
