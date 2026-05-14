import os
from pathlib import Path

import numpy as np
import pandas as pd

from .paths import first_existing


RECOMMENDED_SIMULATOR_COLUMNS = {
    "strategy",
    "attack_status",
    "attack_feasible",
    "victim_reverted",
    "victim_size_bps_of_reserve",
    "frontrun_size_bps_of_reserve",
    "net_profit_bps_of_frontrun",
    "victim_loss_bps_of_fair_out",
    "tx_cost_per_leg",
}


def read_csv(path: Path | None) -> pd.DataFrame:
    if path is None:
        return pd.DataFrame()
    try:
        return pd.read_csv(path)
    except pd.errors.EmptyDataError:
        return pd.DataFrame()


def as_bool(series: pd.Series) -> pd.Series:
    if series.dtype == bool:
        return series
    return series.astype(str).str.lower().isin(["true", "1", "yes"])


def bps(num: pd.Series, den: pd.Series) -> pd.Series:
    den = pd.to_numeric(den, errors="coerce").replace(0, np.nan)
    return (pd.to_numeric(num, errors="coerce") * 10_000 / den).replace(
        [np.inf, -np.inf],
        np.nan,
    )


def normalize_simulator_rows(df: pd.DataFrame, dataset_name: str) -> pd.DataFrame:
    df = df.copy()
    if df.empty:
        df.attrs["dataset_name"] = dataset_name
        df.attrs["legacy_schema"] = False
        df.attrs["missing_recommended_columns"] = sorted(RECOMMENDED_SIMULATOR_COLUMNS)
        return df

    original_columns = set(df.columns)
    df.attrs["dataset_name"] = dataset_name
    df.attrs["legacy_schema"] = not RECOMMENDED_SIMULATOR_COLUMNS.issubset(
        original_columns
    )
    df.attrs["missing_recommended_columns"] = sorted(
        RECOMMENDED_SIMULATOR_COLUMNS - original_columns
    )

    if "strategy" not in df:
        df["strategy"] = "legacy_closed_form"
    if "pool_label" not in df:
        source = df["source"] if "source" in df else pd.Series("", index=df.index)
        df["pool_label"] = np.where(
            source.astype(str).str.contains("real|raydium", case=False, na=False),
            "unknown_real_pool",
            "synthetic",
        )
    if "frontrun_amount" not in df and "optimal_frontrun" in df:
        df["frontrun_amount"] = df["optimal_frontrun"]
    if "victim_amount" not in df and "amount_in" in df:
        df["victim_amount"] = df["amount_in"]
    if "victim_amount_out_no_attack" not in df and "fair_amount_out" in df:
        df["victim_amount_out_no_attack"] = df["fair_amount_out"]
    if "attack_profitable" in df:
        df["attack_profitable"] = as_bool(df["attack_profitable"])
    else:
        df["attack_profitable"] = (
            pd.to_numeric(df.get("attacker_net_profit", 0), errors="coerce") > 0
        )
    if "attack_feasible" not in df:
        if {"victim_extra_slippage_bps", "victim_slippage_tolerance_bps"}.issubset(
            df.columns
        ):
            df["attack_feasible"] = pd.to_numeric(
                df["victim_extra_slippage_bps"],
                errors="coerce",
            ) <= pd.to_numeric(df["victim_slippage_tolerance_bps"], errors="coerce")
        else:
            df["attack_feasible"] = True
    else:
        df["attack_feasible"] = as_bool(df["attack_feasible"])
    if "victim_reverted" not in df:
        df["victim_reverted"] = ~df["attack_feasible"]
    else:
        df["victim_reverted"] = as_bool(df["victim_reverted"])
    if "attack_status" not in df:
        frontrun = pd.to_numeric(
            df["frontrun_amount"]
            if "frontrun_amount" in df
            else pd.Series(np.nan, index=df.index),
            errors="coerce",
        )
        df["attack_status"] = np.select(
            [frontrun.eq(0) | ~df["attack_profitable"], df["attack_profitable"]],
            ["no_profitable_attack", "executed"],
            default="executed",
        )
    if "victim_size_bps_of_reserve" not in df and {
        "victim_amount",
        "pool_reserve_a",
    }.issubset(df.columns):
        df["victim_size_bps_of_reserve"] = bps(
            df["victim_amount"],
            df["pool_reserve_a"],
        ).round()
    if "frontrun_size_bps_of_reserve" not in df and {
        "frontrun_amount",
        "pool_reserve_a",
    }.issubset(df.columns):
        df["frontrun_size_bps_of_reserve"] = bps(
            df["frontrun_amount"],
            df["pool_reserve_a"],
        ).round()
    if "victim_loss_bps_of_fair_out" not in df and {
        "victim_loss_absolute",
        "victim_amount_out_no_attack",
    }.issubset(df.columns):
        df["victim_loss_bps_of_fair_out"] = bps(
            df["victim_loss_absolute"],
            df["victim_amount_out_no_attack"],
        ).round()
    if "tx_cost_per_leg" not in df and "tx_cost_total" in df:
        df["tx_cost_per_leg"] = pd.to_numeric(df["tx_cost_total"], errors="coerce") / 2
    if "net_profit_bps_of_frontrun" not in df and {
        "attacker_net_profit",
        "frontrun_amount",
    }.issubset(df.columns):
        df["net_profit_bps_of_frontrun"] = bps(
            df["attacker_net_profit"],
            df["frontrun_amount"],
        )

    for col in [
        "pool_fee_bps",
        "victim_size_bps_of_reserve",
        "frontrun_size_bps_of_reserve",
        "victim_loss_bps_of_fair_out",
        "victim_extra_slippage_bps",
        "victim_slippage_tolerance_bps",
        "attacker_net_profit",
        "attacker_gross_profit",
        "tx_cost_per_leg",
        "tx_cost_total",
    ]:
        if col in df:
            df[col] = pd.to_numeric(df[col], errors="coerce")

    df["attack_realized"] = df["attack_profitable"] & df["attack_feasible"]
    df["blocker"] = np.select(
        [
            df["attack_realized"],
            ~df["attack_profitable"],
            df["attack_profitable"] & ~df["attack_feasible"],
        ],
        ["realized", "not_profitable", "victim_would_revert"],
        default="unknown",
    )
    return df


def load_inputs(root: Path) -> dict:
    configured_results = os.environ.get("MEV_RESULTS_DIR")
    results = root / "results"
    clmm_results = (
        Path(configured_results).expanduser()
        if configured_results
        else results
    )
    if not clmm_results.is_absolute():
        clmm_results = root / clmm_results
    paths = {
        "synthetic_sweep": first_existing(results, "sweep.csv", "output.csv"),
        "zhou_vs_numerical": first_existing(results, "zhou_vs_numerical.csv"),
        "real_pool_comparison": first_existing(
            results,
            "real_pool_comparison.csv",
            "sweep_real_pool.csv",
        ),
        "historical_cpmm_swaps_status": first_existing(
            results,
            "historical_cpmm_swaps_status.csv",
        ),
        "historical_cpmm_pipeline_summary": first_existing(
            results,
            "historical_cpmm_pipeline_summary.csv",
        ),
        "historical_cpmm_decoded": first_existing(
            results,
            "historical_cpmm_decoded.csv",
        ),
        "historical_cpmm_candidates": first_existing(
            results,
            "historical_cpmm_candidates.csv",
        ),
        "historical_cpmm": first_existing(results, "historical_cpmm_candidates.csv"),
        "historical_clmm_swaps_status": first_existing(
            results,
            "historical_clmm_swaps_status.csv",
        ),
        "historical_clmm_pipeline_summary": first_existing(
            results,
            "historical_clmm_pipeline_summary.csv",
        ),
        "historical_clmm_state_probe": first_existing(
            results,
            "historical_clmm_state_probe.csv",
        ),
        "historical_clmm_decoded": first_existing(
            results,
            "historical_clmm_decoded.csv",
        ),
        "historical_clmm_live_swaps": first_existing(
            clmm_results,
            "historical_clmm_live_swaps.csv",
        ),
        "historical_clmm_live_snapshots": first_existing(
            clmm_results,
            "historical_clmm_live_snapshots.csv",
        ),
        "historical_clmm_live_candidates": first_existing(
            clmm_results,
            "historical_clmm_live_candidates.csv",
        ),
        "historical_clmm_candidates": first_existing(
            clmm_results,
            "historical_clmm_candidates.csv",
        ),
        "historical_clmm": first_existing(clmm_results, "historical_clmm_candidates.csv"),
    }
    frames = {
        "synthetic_sweep": normalize_simulator_rows(
            read_csv(paths["synthetic_sweep"]),
            "synthetic_sweep",
        ),
        "zhou_vs_numerical": read_csv(paths["zhou_vs_numerical"]),
        "real_pool_comparison": normalize_simulator_rows(
            read_csv(paths["real_pool_comparison"]),
            "real_pool_comparison",
        ),
        "historical_cpmm_swaps_status": read_csv(paths["historical_cpmm_swaps_status"]),
        "historical_cpmm_pipeline_summary": read_csv(
            paths["historical_cpmm_pipeline_summary"],
        ),
        "historical_cpmm_decoded": read_csv(paths["historical_cpmm_decoded"]),
        "historical_cpmm_candidates": normalize_simulator_rows(
            read_csv(paths["historical_cpmm_candidates"]),
            "historical_cpmm_candidates",
        ),
        "historical_cpmm": normalize_simulator_rows(
            read_csv(paths["historical_cpmm"]),
            "historical_cpmm",
        ),
        "historical_clmm_swaps_status": read_csv(paths["historical_clmm_swaps_status"]),
        "historical_clmm_pipeline_summary": read_csv(
            paths["historical_clmm_pipeline_summary"],
        ),
        "historical_clmm_state_probe": read_csv(paths["historical_clmm_state_probe"]),
        "historical_clmm_decoded": read_csv(paths["historical_clmm_decoded"]),
        "historical_clmm_live_swaps": read_csv(paths["historical_clmm_live_swaps"]),
        "historical_clmm_live_snapshots": read_csv(
            paths["historical_clmm_live_snapshots"],
        ),
        "historical_clmm_live_candidates": read_csv(
            paths["historical_clmm_live_candidates"],
        ),
        "historical_clmm_candidates": read_csv(paths["historical_clmm_candidates"]),
        "historical_clmm": read_csv(paths["historical_clmm"]),
    }
    return {"paths": paths, "frames": frames}
