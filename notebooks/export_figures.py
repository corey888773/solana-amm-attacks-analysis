from pathlib import Path
import sys

import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns
from matplotlib.ticker import FuncFormatter

for candidate in [Path.cwd(), *Path.cwd().parents]:
    if (candidate / "helpers").exists():
        sys.path.insert(0, str(candidate))
        break
    if (candidate / "notebooks" / "helpers").exists():
        sys.path.insert(0, str(candidate / "notebooks"))
        break

from helpers import find_repo_root, load_inputs


sns.set_theme(style="whitegrid", context="paper", font_scale=1.05)
plt.rcParams.update(
    {
        "figure.dpi": 140,
        "savefig.dpi": 220,
        "axes.titlesize": 13,
        "axes.labelsize": 11,
        "xtick.labelsize": 9,
        "ytick.labelsize": 9,
        "legend.fontsize": 9,
        "axes.titleweight": "semibold",
    }
)


def save(fig, output_dir: Path, name: str) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / f"{name}.png"
    fig.savefig(path, bbox_inches="tight", facecolor="white")
    plt.close(fig)
    print(path)


def compact_number(value) -> str:
    value = float(value)
    if abs(value) >= 1_000_000_000:
        return f"{value / 1_000_000_000:g}B"
    if abs(value) >= 1_000_000:
        return f"{value / 1_000_000:g}M"
    if abs(value) >= 1_000:
        return f"{value / 1_000:g}k"
    return f"{value:g}"


def compact_ticklabels(ax) -> None:
    try:
        xticks = [float(label.get_text().replace("−", "-")) for label in ax.get_xticklabels()]
        ax.set_xticklabels([compact_number(x) for x in xticks], rotation=35, ha="right")
    except ValueError:
        pass
    try:
        yticks = [float(label.get_text().replace("−", "-")) for label in ax.get_yticklabels()]
        ax.set_yticklabels([compact_number(y) for y in yticks], rotation=0)
    except ValueError:
        pass


def save_heatmap(df, output_dir, *, name, value, title, cbar_label, fmt=".2f", cmap="viridis"):
    required = {"victim_size_bps_of_reserve", "pool_fee_bps", value}
    if df.empty or not required.issubset(df.columns):
        return
    heat = df.pivot_table(
        index="victim_size_bps_of_reserve",
        columns="pool_fee_bps",
        values=value,
        aggfunc="mean" if df[value].dtype == bool else "median",
    ).sort_index()
    fig, ax = plt.subplots(figsize=(9.5, 5.8))
    sns.heatmap(
        heat,
        annot=True,
        fmt=fmt,
        cmap=cmap,
        linewidths=0.4,
        linecolor="white",
        cbar_kws={"label": cbar_label},
        ax=ax,
    )
    ax.set_title(title)
    ax.set_xlabel("Pool fee [bps]")
    ax.set_ylabel("Victim size [bps of reserve in]")
    fig.tight_layout()
    save(fig, output_dir, name)


def save_parameter_heatmap(
    df,
    output_dir,
    *,
    name,
    index,
    columns,
    value,
    aggfunc,
    title,
    xlabel,
    ylabel,
    cbar_label,
    fmt=".2f",
    cmap="viridis",
):
    if df.empty or not {index, columns, value}.issubset(df.columns):
        return
    heat = df.pivot_table(index=index, columns=columns, values=value, aggfunc=aggfunc)
    heat = heat.sort_index().sort_index(axis=1)
    fig, ax = plt.subplots(figsize=(9.5, 5.8))
    sns.heatmap(
        heat,
        annot=True,
        fmt=fmt,
        cmap=cmap,
        linewidths=0.4,
        linecolor="white",
        cbar_kws={"label": cbar_label},
        ax=ax,
    )
    ax.set_title(title)
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)
    compact_ticklabels(ax)
    fig.tight_layout()
    save(fig, output_dir, name)


def save_sensitivity(df, output_dir, *, name, x, y, title, xlabel, ylabel):
    if df.empty or not {x, y}.issubset(df.columns) or df[x].nunique() < 2:
        return
    summary = (
        df.groupby(x)
        .agg(value=(y, "mean" if df[y].dtype == bool else "median"))
        .reset_index()
        .sort_values(x)
    )
    fig, ax = plt.subplots(figsize=(8.5, 4.8))
    sns.lineplot(data=summary, x=x, y="value", marker="o", linewidth=2, ax=ax)
    ax.set_title(title)
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)
    ax.grid(True, alpha=0.35)
    fig.tight_layout()
    save(fig, output_dir, name)


def save_profitability_feasibility_tradeoff(sweep, output_dir):
    if sweep.empty:
        return
    summary = (
        sweep.groupby("victim_amount")
        .agg(
            profitable_rate=("attack_profitable", "mean"),
            feasible_rate=("attack_feasible", "mean"),
            median_net_profit=("attacker_net_profit", "median"),
        )
        .reset_index()
        .sort_values("victim_amount")
    )
    fig, ax1 = plt.subplots(figsize=(9, 5))
    sns.lineplot(
        data=summary,
        x="victim_amount",
        y="profitable_rate",
        marker="o",
        linewidth=2,
        label="Profitable",
        ax=ax1,
    )
    sns.lineplot(
        data=summary,
        x="victim_amount",
        y="feasible_rate",
        marker="o",
        linewidth=2,
        label="Feasible",
        ax=ax1,
    )
    ax1.set_xscale("log")
    ax1.set_ylim(-0.03, 1.03)
    ax1.set_title("Profitability vs victim slippage feasibility")
    ax1.set_xlabel("Victim amount [token-in units]")
    ax1.set_ylabel("Rate")
    ax1.legend(frameon=True)
    ax1.grid(True, alpha=0.35)
    fig.tight_layout()
    save(fig, output_dir, "01_profitability_feasibility_tradeoff")


def save_blockers_by_victim_size(sweep, output_dir):
    if sweep.empty or "blocker" not in sweep.columns:
        return
    table = (
        sweep.groupby(["victim_amount", "blocker"])
        .size()
        .unstack(fill_value=0)
        .sort_index()
    )
    table = table.div(table.sum(axis=1), axis=0)
    fig, ax = plt.subplots(figsize=(9, 5))
    table.plot(marker="o", linewidth=2, ax=ax)
    ax.set_xscale("log")
    ax.set_title("Why candidate attacks do not become realized attacks")
    ax.set_xlabel("Victim amount [token-in units]")
    ax.set_ylabel("Share of scenarios")
    ax.legend(title="Outcome", frameon=True, loc="upper left")
    ax.grid(True, alpha=0.35)
    fig.tight_layout()
    save(fig, output_dir, "01_attack_blockers_by_victim_size")


def save_zhou(zhou, output_dir):
    if zhou.empty:
        return
    fig, ax = plt.subplots(figsize=(9, 5))
    sns.lineplot(
        data=zhou,
        x="fee_bps",
        y="profit_gap_abs",
        hue="liquidity_depth_label",
        estimator="median",
        errorbar=None,
        marker="o",
        linewidth=2,
        ax=ax,
    )
    ax.axhline(0, color="black", linewidth=1)
    ax.set_title("Numerical optimizer profit advantage over closed-form baseline")
    ax.set_xlabel("Fee [bps]")
    ax.set_ylabel("Median net profit gap [token-in units]")
    ax.legend(title="Liquidity depth", frameon=True)
    fig.tight_layout()
    save(fig, output_dir, "01_zhou_profit_gap")

    fig, ax = plt.subplots(figsize=(9, 5))
    sns.lineplot(
        data=zhou,
        x="fee_bps",
        y="frontrun_gap_abs",
        hue="liquidity_depth_label",
        estimator="median",
        errorbar=None,
        marker="o",
        linewidth=2,
        ax=ax,
    )
    ax.axhline(0, color="black", linewidth=1)
    ax.set_title("Numerical optimizer frontrun difference vs closed-form baseline")
    ax.set_xlabel("Fee [bps]")
    ax.set_ylabel("Median frontrun gap [token-in units]")
    ax.legend(title="Liquidity depth", frameon=True)
    fig.tight_layout()
    save(fig, output_dir, "01_zhou_frontrun_gap")


def save_cpmm(comparison, output_dir):
    if comparison.empty:
        return
    labels = {
        "synthetic_cpmm_matched_state": "matched synthetic",
        "raydium_cpmm_snapshot": "Raydium snapshot",
    }
    compact = comparison.copy()
    compact["source_label"] = compact["source"].map(labels).fillna(compact["source"])

    fig, ax = plt.subplots(figsize=(7.5, 4.8))
    sns.barplot(data=compact, x="source_label", y="attack_realized", errorbar=None, ax=ax)
    ax.set_title("Raydium CPMM snapshot: realized attack rate")
    ax.set_xlabel("")
    ax.set_ylabel("Profitable and feasible rate")
    ax.set_ylim(0, max(0.3, compact["attack_realized"].mean() * 1.4))
    ax.bar_label(ax.containers[0], fmt="%.2f", padding=3)
    fig.tight_layout()
    save(fig, output_dir, "02_cpmm_snapshot_realized_rate")

    profit = (
        compact.groupby(["source_label", "victim_amount"], dropna=False)
        .agg(median_net_profit=("attacker_net_profit", "median"))
        .reset_index()
    )
    profit["median_net_profit_m"] = profit["median_net_profit"] / 1_000_000
    fig, ax = plt.subplots(figsize=(9, 5.2))
    sns.lineplot(
        data=profit,
        x="victim_amount",
        y="median_net_profit_m",
        hue="source_label",
        marker="o",
        linewidth=2,
        ax=ax,
    )
    ax.axhline(0, color="black", linewidth=1)
    ax.set_xscale("log")
    ax.set_title("Raydium CPMM snapshot: median net profit by victim amount")
    ax.set_xlabel("Victim amount [token-in units]")
    ax.set_ylabel("Median attacker net profit [millions of token-in units]")
    ax.xaxis.set_major_formatter(FuncFormatter(lambda x, _: compact_number(x)))
    ax.legend(frameon=True)
    ax.grid(True, alpha=0.35)
    fig.tight_layout()
    save(fig, output_dir, "02_cpmm_snapshot_net_profit")


def main() -> None:
    root = find_repo_root()
    output_dir = root / "notebooks" / "output" / "figures"
    inputs = load_inputs(root)
    sweep = inputs["frames"]["synthetic_sweep"]
    zhou = inputs["frames"]["zhou_vs_numerical"]
    comparison = inputs["frames"]["real_pool_comparison"]

    save_parameter_heatmap(
        sweep,
        output_dir,
        name="01_profitable_rate_by_victim_and_reserve",
        index="victim_amount",
        columns="pool_reserve_a",
        value="attack_profitable",
        aggfunc="mean",
        title="Profitability rate by victim amount and pool reserve",
        xlabel="Reserve in",
        ylabel="Victim amount",
        cbar_label="profitable rate",
    )
    save_parameter_heatmap(
        sweep,
        output_dir,
        name="01_feasible_rate_by_victim_and_slippage",
        index="victim_amount",
        columns="victim_slippage_tolerance_bps",
        value="attack_feasible",
        aggfunc="mean",
        title="Feasibility rate by victim amount and slippage tolerance",
        xlabel="Victim slippage tolerance [bps]",
        ylabel="Victim amount",
        cbar_label="feasible rate",
    )
    save_parameter_heatmap(
        sweep,
        output_dir,
        name="01_median_net_profit_by_victim_and_reserve",
        index="victim_amount",
        columns="pool_reserve_a",
        value="attacker_net_profit",
        aggfunc="median",
        title="Median net profit by victim amount and pool reserve",
        xlabel="Reserve in",
        ylabel="Victim amount",
        cbar_label="median net profit",
        fmt=".0f",
        cmap="mako",
    )
    save_profitability_feasibility_tradeoff(sweep, output_dir)
    save_blockers_by_victim_size(sweep, output_dir)
    save_sensitivity(
        sweep,
        output_dir,
        name="01_feasibility_vs_slippage",
        x="victim_slippage_tolerance_bps",
        y="attack_feasible",
        title="Feasibility rate vs victim slippage tolerance",
        xlabel="Victim slippage tolerance [bps]",
        ylabel="Feasible rate",
    )
    save_zhou(zhou, output_dir)
    save_cpmm(comparison, output_dir)


if __name__ == "__main__":
    main()
