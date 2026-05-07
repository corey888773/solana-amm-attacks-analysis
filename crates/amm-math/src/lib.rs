pub mod cpmm;
pub mod sandwich;
pub mod types;

pub use cpmm::multi_fee;
pub use cpmm::simple_fee as constant_product;
pub use sandwich::numerical as sandwich_numerical;

/// 1 basis point = 0.01%. 10_000 bps = 100%.
pub const BPS_DENOMINATOR: u128 = 10_000;
pub const BPS_DENOMINATOR_F64: f64 = 10_000.0;
