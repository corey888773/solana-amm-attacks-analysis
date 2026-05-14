import pandas as pd


SUCCESS_LABELS = {
    "accepted",
    "candidate",
    "decoded",
    "evaluated",
    "included",
    "kept",
    "ok",
    "passed",
    "simulated",
    "success",
}

ANALYSIS_EXCLUSION_LABELS = {
    "below_candidate_threshold",
}


def _first_column(df: pd.DataFrame, names: list[str]) -> str | None:
    return next((name for name in names if name in df.columns), None)


def _profit_column(df: pd.DataFrame) -> str | None:
    return _first_column(df, ["attacker_net_profit", "net_profit", "profit"])


def _with_attack_realized(df: pd.DataFrame) -> pd.DataFrame:
    working = df.copy()
    if "attack_realized" not in working and {
        "attack_profitable",
        "attack_feasible",
    }.issubset(working.columns):
        working["attack_realized"] = working["attack_profitable"] & working[
            "attack_feasible"
        ]
    return working


def _bool_series(df: pd.DataFrame, column: str) -> pd.Series:
    if column not in df:
        return pd.Series(False, index=df.index)
    if df[column].dtype == bool:
        return df[column].fillna(False)
    return df[column].astype(str).str.lower().isin(["true", "1", "yes"])


def _analysis_candidates(df: pd.DataFrame) -> pd.DataFrame:
    if df.empty:
        return df.copy()
    if "below_candidate_threshold" in df:
        return df.loc[~_bool_series(df, "below_candidate_threshold")].copy()
    return df.copy()


def historical_pipeline_funnel(frames: dict[str, pd.DataFrame]) -> pd.DataFrame:
    status = frames.get("historical_cpmm_swaps_status", pd.DataFrame())
    decoded = frames.get("historical_cpmm_decoded", pd.DataFrame())
    candidates = frames.get("historical_cpmm_candidates", pd.DataFrame())
    summary = frames.get("historical_cpmm_pipeline_summary", pd.DataFrame())

    if not status.empty or not decoded.empty or not candidates.empty:
        collected = 0
        if {"stage", "output_rows"}.issubset(summary.columns):
            collected = int(
                pd.to_numeric(
                    summary.loc[
                        summary["stage"].eq("collect_signatures"),
                        "output_rows",
                    ],
                    errors="coerce",
                )
                .fillna(0)
                .sum(),
            )
        if collected == 0:
            collected = len(status)

        if "analysis_status" in status:
            reasons = status["analysis_status"].fillna("").astype(str).str.lower()
            below_threshold = int(reasons.isin(ANALYSIS_EXCLUSION_LABELS).sum())
            true_rejections = int(
                (~reasons.isin(SUCCESS_LABELS | ANALYSIS_EXCLUSION_LABELS)).sum(),
            )
        else:
            below_threshold = int(_bool_series(candidates, "below_candidate_threshold").sum())
            true_rejections = 0

        analysis = _analysis_candidates(candidates)
        return pd.DataFrame(
            [
                {"stage": "collected_signatures", "rows": collected},
                {"stage": "decoded_swaps", "rows": len(decoded)},
                {"stage": "evaluated_rows", "rows": len(candidates)},
                {"stage": "analysis_candidates", "rows": len(analysis)},
                {"stage": "excluded_below_threshold", "rows": below_threshold},
                {"stage": "rejected_rows", "rows": true_rejections},
            ],
        )

    if {"stage", "output_rows"}.issubset(summary.columns):
        group_cols = ["stage"]
        out = (
            summary.groupby(group_cols, dropna=False)
            .agg(rows=("output_rows", "sum"), rejected_rows=("rejected_rows", "sum"))
            .reset_index()
        )
        out["rows"] = pd.to_numeric(out["rows"], errors="coerce")
        out["rejected_rows"] = pd.to_numeric(out["rejected_rows"], errors="coerce")
        order = {
            "collect_signatures": 0,
            "fetch_transactions": 1,
            "decode_swaps": 2,
            "build_decoded": 3,
            "evaluate": 4,
        }
        out["_order"] = out["stage"].map(order).fillna(99)
        return out.dropna(subset=["rows"]).sort_values("_order").drop(columns="_order")

    label_col = _first_column(summary, ["stage", "step", "name", "metric"])
    count_col = _first_column(summary, ["rows", "count", "n", "value", "total"])
    if label_col and count_col:
        out = summary[[label_col, count_col]].rename(
            columns={label_col: "stage", count_col: "rows"},
        )
        out["rows"] = pd.to_numeric(out["rows"], errors="coerce")
        return out.dropna(subset=["rows"])

    rejections = historical_rejection_pareto(frames)
    rows = [
        {"stage": "swaps_status_rows", "rows": len(swaps), "note": "raw swap status rows"},
        {"stage": "decoded_rows", "rows": len(decoded), "note": "decoded CPMM swaps"},
        {
            "stage": "rejected_rows",
            "rows": int(rejections["rows"].sum()) if not rejections.empty else 0,
            "note": "non-success status/reason rows",
        },
        {
            "stage": "candidate_rows",
            "rows": len(candidates),
            "note": "counterfactual candidates",
        },
    ]
    return pd.DataFrame(rows)


def historical_rejection_pareto(frames: dict[str, pd.DataFrame]) -> pd.DataFrame:
    status = frames.get("historical_cpmm_swaps_status", pd.DataFrame())
    if status.empty:
        return pd.DataFrame(columns=["reason", "rows", "share", "cumulative_share"])

    if "analysis_status" in status:
        analysis_status = status["analysis_status"].fillna("").astype(str).str.lower()
        rejected_mask = ~analysis_status.isin(SUCCESS_LABELS | ANALYSIS_EXCLUSION_LABELS)
        rejected_rows = status.loc[rejected_mask].copy()
        if rejected_rows.empty:
            return pd.DataFrame(columns=["reason", "rows", "share", "cumulative_share"])
        if "rejection_reason" in rejected_rows:
            reasons = rejected_rows["rejection_reason"].fillna("").astype(str).str.strip()
            reasons = reasons.mask(reasons.eq(""), rejected_rows["analysis_status"].astype(str))
        else:
            reasons = rejected_rows["analysis_status"].astype(str)
        out = reasons.value_counts().rename_axis("reason").reset_index(name="rows")
        out["share"] = out["rows"] / out["rows"].sum()
        out["cumulative_share"] = out["share"].cumsum()
        return out

    reason_col = _first_column(
        status,
        [
            "rejection_reason",
            "reject_reason",
            "failure_reason",
            "reason",
            "status",
            "decode_status",
            "pipeline_status",
        ],
    )
    if reason_col is None:
        return pd.DataFrame(columns=["reason", "rows", "share", "cumulative_share"])

    reasons = status[reason_col].fillna("unknown").astype(str).str.strip()
    reasons = reasons.mask(reasons.eq(""), "unknown")
    rejected = reasons.loc[
        ~reasons.str.lower().isin(SUCCESS_LABELS | ANALYSIS_EXCLUSION_LABELS)
    ]
    if rejected.empty:
        return pd.DataFrame(columns=["reason", "rows", "share", "cumulative_share"])

    out = rejected.value_counts().rename_axis("reason").reset_index(name="rows")
    out["share"] = out["rows"] / out["rows"].sum()
    out["cumulative_share"] = out["share"].cumsum()
    return out


def historical_candidate_summary(df: pd.DataFrame) -> pd.DataFrame:
    if df.empty:
        return pd.DataFrame([{"metric": "evaluated rows", "value": 0}])

    analysis = _analysis_candidates(df)
    working = _with_attack_realized(analysis)
    profit_col = _profit_column(working)
    rows = [
        {"metric": "evaluated rows", "value": len(df)},
        {
            "metric": "excluded below threshold",
            "value": int(_bool_series(df, "below_candidate_threshold").sum()),
        },
        {"metric": "analysis candidates", "value": len(working)},
    ]
    if working.empty:
        return pd.DataFrame(rows)
    for col, label in [
        ("attack_profitable", "profitable rate"),
        ("attack_feasible", "feasible rate"),
        ("attack_realized", "realized rate"),
    ]:
        if col in working:
            rows.append({"metric": label, "value": f"{working[col].mean():.1%}"})
    if profit_col:
        profit = pd.to_numeric(working[profit_col], errors="coerce")
        rows.extend(
            [
                {"metric": "median net profit", "value": profit.median()},
                {"metric": "max net profit", "value": profit.max()},
                {"metric": "positive profit rows", "value": int((profit > 0).sum())},
            ],
        )
    return pd.DataFrame(rows)


def historical_slippage_summary(df: pd.DataFrame) -> pd.DataFrame:
    df = _analysis_candidates(df)
    required = {"victim_extra_slippage_bps", "victim_slippage_tolerance_bps"}
    if df.empty or not required.issubset(df.columns):
        return pd.DataFrame()

    extra = pd.to_numeric(df["victim_extra_slippage_bps"], errors="coerce")
    tolerance = pd.to_numeric(df["victim_slippage_tolerance_bps"], errors="coerce")
    margin = tolerance - extra
    feasible = margin >= 0
    return pd.DataFrame(
        [
            {"metric": "rows with slippage data", "value": int(margin.notna().sum())},
            {"metric": "slippage-feasible rate", "value": f"{feasible.mean():.1%}"},
            {"metric": "median tolerance margin bps", "value": margin.median()},
            {"metric": "min tolerance margin bps", "value": margin.min()},
        ],
    )


def historical_top_candidates(df: pd.DataFrame, limit: int = 10) -> pd.DataFrame:
    df = _analysis_candidates(df)
    if df.empty:
        return pd.DataFrame()
    profit_col = _profit_column(df)
    if profit_col is None:
        return pd.DataFrame()

    columns = [
        "signature",
        "slot",
        "pool",
        "pool_label",
        "victim_amount",
        "amount_in",
        "minimum_amount_out",
        "frontrun_amount",
        "victim_slippage_tolerance_bps",
        "victim_extra_slippage_bps",
        profit_col,
    ]
    existing = [col for col in columns if col in df.columns]
    working = df.copy()
    working[profit_col] = pd.to_numeric(working[profit_col], errors="coerce")
    return working.loc[:, existing].sort_values(profit_col, ascending=False).head(limit)


def historical_takeaway_lines(
    comparison: pd.DataFrame,
    candidates: pd.DataFrame,
    funnel: pd.DataFrame,
    rejections: pd.DataFrame,
) -> list[str]:
    lines = []
    if not comparison.empty and "attack_realized" in comparison:
        lines.append(
            f"- Snapshot-derived CPMM realized rate: `{comparison['attack_realized'].mean():.1%}` over `{len(comparison)}` rows.",
        )
    if not funnel.empty:
        stage_rows = {
            row["stage"]: int(row["rows"])
            for _, row in funnel.iterrows()
            if pd.notna(row.get("rows"))
        }
        if "evaluated_rows" in stage_rows and "analysis_candidates" in stage_rows:
            lines.append(
                f"- Historical CPMM evaluated `{stage_rows['evaluated_rows']}` row(s); `{stage_rows['analysis_candidates']}` passed the analysis threshold.",
            )
        else:
            last_stage = funnel.iloc[-1]
            lines.append(
                f"- Historical CPMM funnel ends at `{last_stage['stage']}` with `{int(last_stage['rows'])}` rows.",
            )
    if not rejections.empty:
        top = rejections.iloc[0]
        lines.append(
            f"- Main historical rejection reason: `{top['reason']}` (`{top['share']:.1%}` of rejected rows).",
        )
    if candidates.empty:
        lines.append("- Historical CPMM candidates are currently empty; rejection/funnel evidence is still reportable.")
    else:
        analysis = _analysis_candidates(candidates)
        profit_col = _profit_column(candidates)
        if profit_col and not analysis.empty:
            profit = pd.to_numeric(analysis[profit_col], errors="coerce")
            lines.append(
                f"- Historical CPMM analysis candidates: `{len(analysis)}` rows, median net profit `{profit.median():,.0f}`.",
            )
    return lines
