use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimConfig {
    pub simulation: SimulationParams,
    pub pool: PoolParams,
    pub victim: VictimParams,
    pub costs: CostParams,
    pub attacker: AttackerParams,
    #[serde(default)]
    pub sweep: Option<SweepParams>,
    #[serde(default)]
    pub real_pool: Option<RealPoolParams>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RealPoolParams {
    /// Path (relative to current working dir, typically the workspace root)
    /// to the snapshot directory containing `manifest.json` plus the cached
    /// vault and AmmConfig account JSONs.
    pub manifest: String,
    /// If true, a missing/broken snapshot falls back to synthetic `[pool]`
    /// params. Default is fail-fast because real-pool sweeps should not
    /// silently produce synthetic CSVs.
    #[serde(default)]
    pub allow_synthetic_fallback: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationParams {
    #[serde(default = "default_iterations")]
    pub num_iterations: u32,
    #[serde(default = "default_seed")]
    pub seed: u64, // TODO: use for randomized victim amounts
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolParams {
    pub initial_reserve_a: u64,
    pub initial_reserve_b: u64,
    pub fee_bps: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VictimParams {
    pub swap_amount: u64,
    #[serde(default = "default_slippage")]
    pub slippage_tolerance_bps: u16,
    #[serde(default = "default_direction")]
    pub direction: SwapDirection, // TODO: support B→A swaps in engine
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SwapDirection {
    #[default]
    AToB,
    BToA,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostParams {
    #[serde(default)]
    pub base_fee_lamports: u64,
    #[serde(default)]
    pub priority_fee_lamports: u64,
    #[serde(default)]
    pub jito_tip_lamports: u64,
    /// Price of 1 SOL expressed in units of the input token (the token the attacker pays in).
    /// Used to convert lamport-denominated transaction costs into input-token units so they
    /// can be compared against `gross_profit` on the same scale. User must set this based on
    /// which token is being swapped in (e.g. if input token is USDC at 6 decimals and SOL ≈
    /// 150 USDC, set this to 150.0).
    #[serde(default = "default_input_token_per_sol")]
    pub input_token_per_sol: f64,
}

impl CostParams {
    /// Sum of all lamport-denominated fees (base + priority + Jito tip), in lamports.
    pub fn total_lamports(&self) -> u64 {
        self.base_fee_lamports + self.priority_fee_lamports + self.jito_tip_lamports
    }

    /// Convert total lamport cost into input-token units (smallest denomination of token_in).
    ///
    /// Formula: `(total_lamports / 1e9) * input_token_per_sol`, cast to u64.
    /// Note: 1 SOL = 1_000_000_000 lamports. `input_token_per_sol` is the price of 1 SOL
    /// denominated in the input token's base units (already accounting for that token's
    /// decimals where applicable — see config comment).
    pub fn total_in_input_token(&self) -> u64 {
        let lamports = self.total_lamports() as f64;
        (lamports / 1_000_000_000.0 * self.input_token_per_sol) as u64
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttackerParams {
    #[serde(default = "default_strategy")]
    pub strategy: AttackerStrategy,
    #[serde(default)]
    pub fixed_frontrun_amount: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AttackerStrategy {
    #[default]
    ClosedForm,
    Numerical,
    Fixed,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepParams {
    #[serde(default)]
    pub parallel: bool,
    #[serde(default)]
    pub pool_reserve_a: Option<Vec<u64>>,
    #[serde(default)]
    pub pool_fee_bps: Option<Vec<u16>>,
    #[serde(default)]
    pub victim_swap_amount: Option<Vec<u64>>,
    #[serde(default)]
    pub victim_slippage_bps: Option<Vec<u16>>,
}

fn default_iterations() -> u32 {
    1
}
fn default_seed() -> u64 {
    42
}
fn default_slippage() -> u16 {
    300
}
fn default_direction() -> SwapDirection {
    SwapDirection::AToB
}
fn default_strategy() -> AttackerStrategy {
    AttackerStrategy::ClosedForm
}
fn default_input_token_per_sol() -> f64 {
    1.0
}

impl SimConfig {
    /// Load config: TOML file > env vars (MEV_*) > defaults
    pub fn load(path: Option<&str>) -> Result<Self, figment::Error> {
        let mut figment = Figment::new();

        if let Some(p) = path {
            figment = figment.merge(Toml::file(p));
        }

        figment.merge(Env::prefixed("MEV_").split("_")).extract()
    }
}
