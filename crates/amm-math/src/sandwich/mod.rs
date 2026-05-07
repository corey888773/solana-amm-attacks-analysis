//! Sandwich attack sizing models.
//!
//! `closed_form` keeps the Zhou et al. baseline. `numerical` searches the
//! actual integer CPMM profit surface and is the candidate production model
//! for fee-aware simulations.

pub mod closed_form;
pub mod numerical;

pub use closed_form::compute_closed_form_sandwich;
pub use numerical::compute_numerical_sandwich;
