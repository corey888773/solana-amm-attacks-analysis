pub mod constant_product;
pub mod sandwich;
pub mod types;

/// 1 basis point = 0.01%. 10_000 bps = 100%.
pub const BPS_DENOMINATOR: u128 = 10_000;
pub const BPS_DENOMINATOR_F64: f64 = 10_000.0;
