import pandas as pd


def data_readiness(root, inputs: dict, dataset_names: list[str]) -> pd.DataFrame:
    rows = []
    for name in dataset_names:
        df = inputs["frames"][name]
        path = inputs["paths"].get(name)
        legacy = bool(df.attrs.get("legacy_schema", False))
        missing_columns = df.attrs.get("missing_recommended_columns", [])
        rows.append(
            {
                "dataset": name,
                "file": str(path.relative_to(root)) if path else "missing",
                "rows": len(df),
                "columns": len(df.columns),
                "ready_for_final_numbers": bool(len(df) > 0 and not legacy),
                "note": "legacy schema: regenerate CSV"
                if legacy
                else (f"missing: {', '.join(missing_columns)}" if len(df) == 0 else "ok"),
            }
        )
    return pd.DataFrame(rows)


def hypothesis_scorecard(df: pd.DataFrame) -> pd.DataFrame:
    if df.empty:
        return pd.DataFrame()

    successful = df.loc[df["attack_realized"]]
    victim_loss = (
        f"{successful['victim_loss_bps_of_fair_out'].median():,.0f}"
        if not successful.empty and "victim_loss_bps_of_fair_out" in successful
        else "n/a"
    )

    return pd.DataFrame(
        [
            {
                "question": "Czy atak zarabia przed slippage filter?",
                "metric": "profitable rows",
                "value": f"{df['attack_profitable'].mean():.1%}",
            },
            {
                "question": "Czy ofiara nie revertuje?",
                "metric": "feasible rows",
                "value": f"{df['attack_feasible'].mean():.1%}",
            },
            {
                "question": "Czy atak jest ekonomicznie realizowalny?",
                "metric": "profitable and feasible rows",
                "value": f"{df['attack_realized'].mean():.1%}",
            },
            {
                "question": "Typowy wynik attackera",
                "metric": "median net profit",
                "value": f"{df['attacker_net_profit'].median():,.0f}",
            },
            {
                "question": "Typowa strata ofiary w udanych przypadkach",
                "metric": "median victim loss bps",
                "value": victim_loss,
            },
        ]
    )


def blocker_table(df: pd.DataFrame) -> pd.DataFrame:
    if df.empty:
        return pd.DataFrame()

    return (
        df.groupby("blocker")
        .agg(
            rows=("blocker", "size"),
            share=("blocker", lambda s: len(s) / len(df)),
            median_net_profit=("attacker_net_profit", "median"),
            median_victim_loss_bps=("victim_loss_bps_of_fair_out", "median"),
        )
        .reset_index()
        .sort_values("rows", ascending=False)
    )


def best_conditions(df: pd.DataFrame, limit: int = 12) -> pd.DataFrame:
    if df.empty or not df["attack_realized"].any():
        return pd.DataFrame()

    columns = [
        "source",
        "pool_label",
        "strategy",
        "pool_fee_bps",
        "victim_amount",
        "victim_size_bps_of_reserve",
        "victim_slippage_tolerance_bps",
        "frontrun_amount",
        "attacker_net_profit",
        "victim_loss_bps_of_fair_out",
        "tx_cost_per_leg",
    ]
    existing = [col for col in columns if col in df.columns]
    return (
        df.loc[df["attack_realized"], existing]
        .sort_values("attacker_net_profit", ascending=False)
        .head(limit)
    )


def historical_counterfactual_summary(df: pd.DataFrame) -> pd.DataFrame:
    if df.empty:
        return pd.DataFrame()

    working = df.copy()
    if "attack_realized" not in working and {
        "attack_profitable",
        "attack_feasible",
    }.issubset(working.columns):
        working["attack_realized"] = working["attack_profitable"] & working[
            "attack_feasible"
        ]

    group_cols = [col for col in ["pool_type", "pool_label", "inclusion_reason"] if col in working]
    if not group_cols:
        group_cols = ["pool_label"] if "pool_label" in working else []

    if not group_cols:
        return pd.DataFrame()

    profit_col = "attacker_net_profit" if "attacker_net_profit" in working else "net_profit"
    return (
        working.groupby(group_cols, dropna=False)
        .agg(
            rows=(group_cols[0], "size"),
            profitable_rate=("attack_profitable", "mean"),
            feasible_rate=("attack_feasible", "mean"),
            realized_rate=("attack_realized", "mean"),
            median_net_profit=(profit_col, "median"),
        )
        .reset_index()
    )
