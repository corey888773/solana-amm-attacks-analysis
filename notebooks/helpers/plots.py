import matplotlib.pyplot as plt
import seaborn as sns


def plot_realized_heatmap(df, *, value, title, cbar_label, fmt=".2f", cmap="viridis"):
    if df.empty:
        return None
    required = {"victim_size_bps_of_reserve", "pool_fee_bps", value}
    if not required.issubset(df.columns):
        return None

    heat = df.pivot_table(
        index="victim_size_bps_of_reserve",
        columns="pool_fee_bps",
        values=value,
        aggfunc="mean" if df[value].dtype == bool else "median",
    ).sort_index()
    fig, ax = plt.subplots(figsize=(9, 5))
    sns.heatmap(heat, annot=True, fmt=fmt, cmap=cmap, cbar_kws={"label": cbar_label}, ax=ax)
    ax.set_title(title)
    ax.set_xlabel("pool fee [bps]")
    ax.set_ylabel("victim size [bps of reserve_in]")
    fig.tight_layout()
    return fig


def plot_sensitivity_lines(df, *, x, y, title, ylabel):
    if df.empty or not {x, y}.issubset(df.columns) or df[x].nunique() < 2:
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
