use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SimConfig {
    pub simulation: SimulationParams,
    pub pool: PoolParams,
    pub victim: VictimParams,
    pub costs: CostParams,
    pub attacker: AttackerParams,
    #[serde(default)]
    pub sweep: Option<SweepParams>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulationParams {
    #[serde(default = "default_iterations")]
    pub num_iterations: u32,
    #[serde(default = "default_seed")]
    pub seed: u64, // TODO: use for randomized victim amounts
}

#[derive(Debug, Clone, Deserialize)]
pub struct PoolParams {
    pub initial_reserve_a: u64,
    pub initial_reserve_b: u64,
    pub fee_bps: u16,
}

#[derive(Debug, Clone, Deserialize)]
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
pub struct CostParams {
    #[serde(default)]
    pub base_fee_lamports: u64,
    #[serde(default)]
    pub priority_fee_lamports: u64,
    #[serde(default)]
    pub jito_tip_lamports: u64,
}

impl CostParams {
    pub fn total(&self) -> u64 {
        self.base_fee_lamports + self.priority_fee_lamports + self.jito_tip_lamports
    }
}

#[derive(Debug, Clone, Deserialize)]
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
    Optimal,
    Fixed,
}

#[derive(Debug, Clone, Deserialize)]
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
    AttackerStrategy::Optimal
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
