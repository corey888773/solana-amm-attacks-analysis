from .loading import load_inputs, normalize_simulator_rows
from .historical import (
    historical_candidate_summary,
    historical_pipeline_funnel,
    historical_rejection_pareto,
    historical_slippage_summary,
    historical_takeaway_lines,
    historical_top_candidates,
)
from .metrics import (
    best_conditions,
    blocker_table,
    data_readiness,
    historical_counterfactual_summary,
    hypothesis_scorecard,
)
from .paths import find_repo_root
from .plots import (
    plot_profit_distribution,
    plot_realized_heatmap,
    plot_rejection_pareto,
    plot_sensitivity_lines,
    plot_size_vs_profit,
    plot_slippage_feasibility,
)

__all__ = [
    "best_conditions",
    "blocker_table",
    "data_readiness",
    "find_repo_root",
    "historical_candidate_summary",
    "historical_counterfactual_summary",
    "historical_pipeline_funnel",
    "historical_rejection_pareto",
    "historical_slippage_summary",
    "historical_takeaway_lines",
    "historical_top_candidates",
    "hypothesis_scorecard",
    "load_inputs",
    "normalize_simulator_rows",
    "plot_profit_distribution",
    "plot_realized_heatmap",
    "plot_rejection_pareto",
    "plot_sensitivity_lines",
    "plot_size_vs_profit",
    "plot_slippage_feasibility",
]
