import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns


def _varies(series) -> bool:
    values = pd.Series(series).dropna()
    return len(values) > 1 and values.nunique() > 1


def _profit_column(df):
    for col in ["attacker_net_profit", "net_profit", "profit"]:
        if col in df.columns:
            return col
    return None


def _analysis_rows(df):
    if "below_candidate_threshold" not in df:
        return df
    below = df["below_candidate_threshold"]
    if below.dtype != bool:
        below = below.astype(str).str.lower().isin(["true", "1", "yes"])
    return df.loc[~below.fillna(False)]


def _signed_log10(series):
    values = pd.to_numeric(series, errors="coerce")
    return np.sign(values) * np.log10(np.abs(values) + 1)


def plot_realized_heatmap(df, *, value, title, cbar_label, fmt=".2f", cmap="viridis"):
    if df.empty:
        return None
    required = {"victim_size_bps_of_reserve", "pool_fee_bps", value}
    if not required.issubset(df.columns):
        return None
    if not _varies(df["victim_size_bps_of_reserve"]) and not _varies(df["pool_fee_bps"]):
        return None
    if not _varies(df[value]):
        return None

    heat = df.pivot_table(
        index="victim_size_bps_of_reserve",
        columns="pool_fee_bps",
        values=value,
        aggfunc="mean" if df[value].dtype == bool else "median",
    ).sort_index()
    if heat.dropna(how="all").empty or heat.stack().dropna().nunique() < 2:
        return None

    fig, ax = plt.subplots(figsize=(9, 5))
    sns.heatmap(heat, annot=True, fmt=fmt, cmap=cmap, cbar_kws={"label": cbar_label}, ax=ax)
    ax.set_title(title)
    ax.set_xlabel("pool fee [bps]")
    ax.set_ylabel("victim size [bps of reserve_in]")
    fig.tight_layout()
    return fig


def plot_sensitivity_lines(df, *, x, y, title, ylabel):
    if df.empty or not {x, y}.issubset(df.columns) or not _varies(df[x]) or not _varies(df[y]):
        return None

    summary = (
        df.groupby(x)
        .agg(value=(y, "mean" if df[y].dtype == bool else "median"))
        .reset_index()
        .sort_values(x)
    )
    if summary["value"].nunique() < 2:
        return None

    fig, ax = plt.subplots(figsize=(8, 4.5))
    sns.lineplot(data=summary, x=x, y="value", marker="o", ax=ax)
    ax.set_title(title)
    ax.set_xlabel(x)
    ax.set_ylabel(ylabel)
    fig.tight_layout()
    return fig


def plot_rejection_pareto(rejections):
    if rejections.empty or not {"reason", "rows"}.issubset(rejections.columns):
        return None
    if len(rejections) < 2 or not _varies(rejections["rows"]):
        return None

    fig, ax = plt.subplots(figsize=(9, 4.5))
    sns.barplot(data=rejections, x="rows", y="reason", color="#4c78a8", ax=ax)
    ax.set_title("Historical CPMM rejection Pareto")
    ax.set_xlabel("rows")
    ax.set_ylabel("")
    fig.tight_layout()
    return fig


def plot_profit_distribution(df):
    df = _analysis_rows(df)
    profit_col = _profit_column(df)
    if df.empty or profit_col is None or not _varies(df[profit_col]):
        return None

    working = df.copy()
    working[profit_col] = pd.to_numeric(working[profit_col], errors="coerce")
    working = working.dropna(subset=[profit_col]).sort_values(profit_col)
    working["candidate_rank"] = range(1, len(working) + 1)
    working["signed_log_profit"] = _signed_log10(working[profit_col])

    fig, ax = plt.subplots(figsize=(9, 5))
    sns.scatterplot(data=working, x="signed_log_profit", y="candidate_rank", ax=ax)
    ax.axvline(0, color="black", linewidth=1)
    ax.set_title("Historical CPMM counterfactual net profit by candidate")
    ax.set_xlabel("signed log10(net profit + 1)")
    ax.set_ylabel("candidate rank by net profit")
    fig.tight_layout()
    return fig


def plot_size_vs_profit(df):
    df = _analysis_rows(df)
    profit_col = _profit_column(df)
    size_col = next(
        (
            col
            for col in ["victim_size_bps_of_reserve", "victim_amount", "amount_in"]
            if col in df.columns
        ),
        None,
    )
    if df.empty or profit_col is None or size_col is None:
        return None
    if not _varies(df[size_col]) or not _varies(df[profit_col]):
        return None

    fig, ax = plt.subplots(figsize=(8, 5))
    working = df.copy()
    working["signed_log_profit"] = _signed_log10(working[profit_col])
    sns.scatterplot(data=working, x=size_col, y="signed_log_profit", ax=ax)
    ax.axhline(0, color="black", linewidth=1)
    ax.set_title("Historical CPMM size vs profit")
    ax.set_xlabel(size_col)
    ax.set_ylabel("signed log10(net profit + 1)")
    fig.tight_layout()
    return fig


def plot_slippage_feasibility(df):
    df = _analysis_rows(df)
    required = {"victim_extra_slippage_bps", "victim_slippage_tolerance_bps"}
    if df.empty or not required.issubset(df.columns):
        return None
    if not _varies(df["victim_extra_slippage_bps"]) and not _varies(
        df["victim_slippage_tolerance_bps"],
    ):
        return None

    working = df.copy()
    working["slippage_margin_bps"] = pd.to_numeric(
        working["victim_slippage_tolerance_bps"],
        errors="coerce",
    ) - pd.to_numeric(working["victim_extra_slippage_bps"], errors="coerce")
    if not _varies(working["slippage_margin_bps"]):
        return None

    fig, ax = plt.subplots(figsize=(8, 5))
    if _varies(working["victim_slippage_tolerance_bps"]):
        working["attack_feasible"] = working["slippage_margin_bps"] >= 0
        sns.scatterplot(
            data=working,
            x="victim_slippage_tolerance_bps",
            y="victim_extra_slippage_bps",
            hue="attack_feasible",
            ax=ax,
        )
        ax.plot(ax.get_xlim(), ax.get_xlim(), color="black", linewidth=1)
        ax.set_xlabel("victim slippage tolerance [bps]")
    else:
        working = working.sort_values("victim_extra_slippage_bps").reset_index(drop=True)
        working["candidate_rank"] = working.index + 1
        working["attack_feasible"] = working["slippage_margin_bps"] >= 0
        sns.scatterplot(
            data=working,
            x="candidate_rank",
            y="victim_extra_slippage_bps",
            hue="attack_feasible",
            ax=ax,
        )
        threshold = working["victim_slippage_tolerance_bps"].iloc[0]
        ax.axhline(threshold, color="black", linewidth=1, label="victim tolerance")
        ax.set_xlabel("candidate rank by extra slippage")
    ax.set_title("Historical CPMM slippage feasibility")
    ax.set_ylabel("extra slippage under attack [bps]")
    fig.tight_layout()
    return fig
