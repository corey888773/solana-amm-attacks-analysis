#!/usr/bin/env python3
"""Regenerate thesis/figures/empirical/*.png from the 24h Helius samples.

Reads:
    results/cpmm_24h_sunday/historical_cpmm_candidates_with_real_feas.csv
    results/clmm_24h_sunday/historical_clmm_candidates_with_real_feas.csv

Writes:
    thesis/figures/empirical/cpmm_net_profit_by_candidate.png
    thesis/figures/empirical/cpmm_rejection_pareto.png
    thesis/figures/empirical/cpmm_size_vs_profit.png
    thesis/figures/empirical/clmm_replay_error.png
    thesis/figures/empirical/clmm_attack_outcomes.png
    thesis/figures/empirical/clmm_frontrun_vs_victim.png
    thesis/figures/empirical/clmm_slippage_breach.png

Run from notebooks/ to use the project venv:
    cd notebooks && .venv/bin/python ../scripts/refresh_thesis_figs.py
"""

import csv
from collections import Counter
from pathlib import Path

import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "thesis" / "figures" / "empirical"
OUT.mkdir(parents=True, exist_ok=True)

CPMM = ROOT / "results" / "cpmm_24h_sunday" / "historical_cpmm_candidates_with_real_feas.csv"
CLMM = ROOT / "results" / "clmm_24h_sunday" / "historical_clmm_candidates_with_real_feas.csv"

plt.rcParams.update({"figure.dpi": 130, "font.size": 10})


def load(path: Path) -> list[dict]:
    return list(csv.DictReader(open(path)))


def to_int(v, default=0):
    if v in (None, "", "None"):
        return default
    try:
        return int(float(v))
    except (TypeError, ValueError):
        return default


def net_profit_by_candidate(rows, family: str, out_name: str):
    evald = [r for r in rows if r["model_status"] == "evaluated"]
    profits = [(to_int(r.get("attacker_net_profit")), r.get("actually_feasible") == "true") for r in evald]
    profits.sort(key=lambda x: -x[0])
    fig, ax = plt.subplots(figsize=(7.5, 4.0))
    pos = [(i, p) for i, (p, _) in enumerate(profits) if p > 0]
    zer = [(i, p) for i, (p, _) in enumerate(profits) if p <= 0]
    feas_x = [i for i, (p, f) in enumerate(profits) if p > 0 and f]
    feas_y = [p for i, (p, f) in enumerate(profits) if p > 0 and f]
    math_x = [i for i, (p, f) in enumerate(profits) if p > 0 and not f]
    math_y = [p for i, (p, f) in enumerate(profits) if p > 0 and not f]
    n_prof = sum(1 for p, _ in profits if p > 0)
    xlim = max(n_prof + 20, 120)
    if zer:
        ax.scatter([i for i, _ in zer if i < xlim], [1 for i, _ in zer if i < xlim], s=2, color="#999", alpha=0.4, label=f"unprofitable ({len(zer)}; first {xlim - n_prof} shown)")
    if math_x:
        ax.scatter(math_x, math_y, s=18, color="#d97757", marker="x", linewidth=0.9, alpha=0.7, label=f"math-only ({len(math_x)})")
    if feas_x:
        ax.scatter(feas_x, feas_y, s=18, color="#2c8a3e", alpha=0.6, edgecolor="white", linewidth=0.3, label=f"actually-feasible ({len(feas_x)})")
    ax.set_xlim(-2, xlim)
    ax.set_yscale("log")
    ax.set_ylim(bottom=0.5)
    ax.set_xlabel("Candidate rank (descending net profit)")
    ax.set_ylabel("Attacker net profit [lamports, log]")
    ax.set_title(f"{family} 24h Sunday: net profit per candidate (n={len(evald)}, top {xlim} shown)")
    ax.legend(loc="upper right", fontsize=8)
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(OUT / out_name, dpi=140)
    plt.close(fig)


def cpmm_rejection_pareto(rows):
    cnt = Counter()
    for r in rows:
        reason = r.get("rejection_reason") or "(none)"
        if r["model_status"] == "evaluated" and r["attack_profitable"] == "true":
            cnt["profitable"] += 1
        else:
            cnt[reason] += 1
    items = sorted(cnt.items(), key=lambda x: -x[1])
    labels = [k for k, _ in items]
    values = [v for _, v in items]
    fig, ax = plt.subplots(figsize=(7.5, 4.0))
    bars = ax.bar(range(len(labels)), values, color="#4a6da7")
    ax.set_xticks(range(len(labels)))
    ax.set_xticklabels(labels, rotation=25, ha="right", fontsize=8)
    ax.set_yscale("log")
    ax.set_ylim(bottom=0.5)
    ax.set_ylabel("Count [log]")
    ax.set_title(f"CPMM 24h Sunday: rejection / outcome Pareto (n={sum(values)})")
    for bar, v in zip(bars, values):
        ax.text(bar.get_x() + bar.get_width() / 2, max(v, 1), str(v),
                ha="center", va="bottom", fontsize=8)
    ax.grid(True, axis="y", which="both", alpha=0.3)
    fig.tight_layout()
    fig.savefig(OUT / "cpmm_rejection_pareto.png", dpi=140)
    plt.close(fig)


def size_vs_profit(rows, family: str, out_name: str):
    evald = [r for r in rows if r["model_status"] == "evaluated"]
    feas = [(to_int(r.get("amount_in")), to_int(r.get("attacker_net_profit"))) for r in evald
            if r.get("attack_profitable") == "true" and r.get("actually_feasible") == "true"]
    inf = [(to_int(r.get("amount_in")), to_int(r.get("attacker_net_profit"))) for r in evald
           if r.get("attack_profitable") == "true" and r.get("actually_feasible") != "true"]
    unp = [(to_int(r.get("amount_in")), 0) for r in evald if r.get("attack_profitable") != "true"]
    fig, ax = plt.subplots(figsize=(7.5, 4.0))
    if unp:
        ax.scatter([x for x, _ in unp], [1 for _ in unp], s=2, color="#bbb", alpha=0.25, label=f"unprofitable ({len(unp)})")
    if inf:
        ax.scatter([x for x, _ in inf], [y for _, y in inf], s=18, color="#d97757", marker="x", linewidth=0.9, alpha=0.7, label=f"math-only ({len(inf)})")
    if feas:
        ax.scatter([x for x, _ in feas], [y for _, y in feas], s=18, color="#2c8a3e", alpha=0.6, edgecolor="white", linewidth=0.3, label=f"actually-feasible ({len(feas)})")
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_ylim(bottom=0.5)
    ax.set_xlabel("Victim amount_in [base units, log]")
    ax.set_ylabel("Attacker net profit [lamports, log]")
    ax.set_title(f"{family} 24h Sunday: victim size vs attacker profit (n={len(evald)})")
    ax.legend(loc="upper left", fontsize=8)
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(OUT / out_name, dpi=140)
    plt.close(fig)


def clmm_replay_error(rows):
    evald = [r for r in rows if r["model_status"] == "evaluated"]
    err = [to_int(r.get("replay_error_bps")) for r in evald if r.get("replay_error_bps") not in ("", "None", None)]
    fig, ax = plt.subplots(figsize=(7.0, 4.0))
    bins = np.linspace(0, max(max(err), 100), 60)
    ax.hist(err, bins=bins, color="#4a6da7", edgecolor="white", linewidth=0.4)
    ax.axvline(100, color="#d22", linestyle="--", linewidth=1, label="100 bps tolerance")
    med = int(np.median(err))
    p95 = int(np.percentile(err, 95))
    mx = max(err)
    ax.set_xlabel("replay_error_bps")
    ax.set_ylabel("Number of evaluated rows")
    ax.set_title(f"CLMM 24h Sunday: replay error distribution (n={len(err)}; med={med}, p95={p95}, max={mx})")
    ax.legend(fontsize=8)
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(OUT / "clmm_replay_error.png", dpi=140)
    plt.close(fig)


def clmm_attack_outcomes(rows):
    pools = sorted({r["pool_label"] for r in rows if r.get("pool_label")})
    cats = ["profitable_actually_feasible", "profitable_math_only", "unprofitable", "rejected"]
    counts = {p: {c: 0 for c in cats} for p in pools}
    for r in rows:
        p = r.get("pool_label")
        if not p:
            continue
        if r["model_status"] != "evaluated":
            counts[p]["rejected"] += 1
        elif r["attack_profitable"] == "true":
            if r.get("actually_feasible") == "true":
                counts[p]["profitable_actually_feasible"] += 1
            else:
                counts[p]["profitable_math_only"] += 1
        else:
            counts[p]["unprofitable"] += 1
    fig, ax = plt.subplots(figsize=(8.5, 4.5))
    colors = {"profitable_actually_feasible": "#2c8a3e", "profitable_math_only": "#d97757",
              "unprofitable": "#aaaaaa", "rejected": "#666666"}
    x = np.arange(len(pools))
    width = 0.20
    offsets = {cat: (i - 1.5) * width for i, cat in enumerate(cats)}
    for cat in cats:
        vals = np.array([counts[p][cat] for p in pools])
        bars = ax.bar(x + offsets[cat], vals, width=width, color=colors[cat],
                      label=cat.replace("_", " "), edgecolor="white", linewidth=0.5)
        for bar, v in zip(bars, vals):
            if v > 0:
                ax.text(bar.get_x() + bar.get_width() / 2, max(v, 1), str(int(v)),
                        ha="center", va="bottom", fontsize=7)
    ax.set_yscale("log")
    ax.set_ylim(bottom=0.5)
    ax.set_xticks(x)
    ax.set_xticklabels(pools, rotation=10, ha="right", fontsize=9)
    ax.set_ylabel("Row count [log]")
    ax.set_title("CLMM 24h Sunday: attack outcomes by pool")
    ax.legend(loc="upper right", fontsize=8)
    ax.grid(True, axis="y", which="both", alpha=0.3)
    fig.tight_layout()
    fig.savefig(OUT / "clmm_attack_outcomes.png", dpi=140)
    plt.close(fig)


def clmm_frontrun_vs_victim(rows):
    feas = [(to_int(r.get("amount_in")), to_int(r.get("optimal_frontrun"))) for r in rows
            if r.get("attack_profitable") == "true" and r.get("actually_feasible") == "true"
            and to_int(r.get("amount_in")) > 0 and to_int(r.get("optimal_frontrun")) > 0]
    fig, ax = plt.subplots(figsize=(7.0, 5.0))
    xs = [x for x, _ in feas]
    ys = [y for _, y in feas]
    ax.scatter(xs, ys, s=18, color="#2c8a3e", alpha=0.6, edgecolor="white", linewidth=0.3)
    lo = min(min(xs), min(ys))
    hi = max(max(xs), max(ys))
    ax.plot([lo, hi], [lo, hi], "--", color="#999", linewidth=1, label="frontrun = victim")
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xlabel("victim amount_in [base units, log]")
    ax.set_ylabel("attacker optimal_frontrun [base units, log]")
    ax.set_title(f"CLMM 24h Sunday: frontrun vs victim on actually-feasible profitable rows (n={len(feas)})")
    ax.legend(fontsize=8)
    ax.grid(True, which="both", alpha=0.3)
    fig.tight_layout()
    fig.savefig(OUT / "clmm_frontrun_vs_victim.png", dpi=140)
    plt.close(fig)


def clmm_slippage_breach(rows):
    feas = [to_int(r.get("victim_extra_slippage_bps")) for r in rows
            if r.get("attack_profitable") == "true" and r.get("actually_feasible") == "true"]
    fig, ax = plt.subplots(figsize=(7.0, 4.0))
    bins = np.logspace(0, np.log10(max(max(feas), 100)), 50)
    ax.hist(feas, bins=bins, color="#4a6da7", edgecolor="white", linewidth=0.4)
    ax.axvline(100, color="#d22", linestyle="--", linewidth=1, label="100 bps cap (mainstream default)")
    ax.axvline(50, color="#22d", linestyle=":", linewidth=1, label="50 bps cap")
    ax.set_xscale("log")
    ax.set_xlabel("victim_extra_slippage_bps [log]")
    ax.set_ylabel("count of actually-feasible profitable rows")
    ax.set_title(f"CLMM 24h Sunday: victim extra slippage on profitable rows (n={len(feas)})")
    ax.legend(fontsize=8)
    ax.grid(True, which="both", alpha=0.3)
    fig.tight_layout()
    fig.savefig(OUT / "clmm_slippage_breach.png", dpi=140)
    plt.close(fig)


def main():
    print(f"reading {CPMM}")
    cpmm_rows = load(CPMM)
    print(f"  {len(cpmm_rows)} rows")
    net_profit_by_candidate(cpmm_rows, "CPMM", "cpmm_net_profit_by_candidate.png")
    cpmm_rejection_pareto(cpmm_rows)
    size_vs_profit(cpmm_rows, "CPMM", "cpmm_size_vs_profit.png")

    print(f"reading {CLMM}")
    clmm_rows = load(CLMM)
    print(f"  {len(clmm_rows)} rows")
    clmm_replay_error(clmm_rows)
    clmm_attack_outcomes(clmm_rows)
    clmm_frontrun_vs_victim(clmm_rows)
    clmm_slippage_breach(clmm_rows)
    net_profit_by_candidate(clmm_rows, "CLMM", "clmm_net_profit_by_candidate.png")
    size_vs_profit(clmm_rows, "CLMM", "clmm_size_vs_profit.png")

    print(f"\nwrote 9 figures to {OUT}/")


if __name__ == "__main__":
    main()
