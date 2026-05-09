use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    Included,
    TxFailed,
    NoCpmmSwap,
    UnsupportedSwapInstruction,
    DecodeFailed,
    MissingPreState,
    MissingTokenBalances,
    MultiSwapAmbiguous,
    VaultMismatch,
    AmountMismatch,
    InvalidFeeConfig,
    BelowCandidateThreshold,
}

impl AnalysisStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Included => "included",
            Self::TxFailed => "tx_failed",
            Self::NoCpmmSwap => "no_cpmm_swap",
            Self::UnsupportedSwapInstruction => "unsupported_swap_instruction",
            Self::DecodeFailed => "decode_failed",
            Self::MissingPreState => "missing_pre_state",
            Self::MissingTokenBalances => "missing_token_balances",
            Self::MultiSwapAmbiguous => "multi_swap_ambiguous",
            Self::VaultMismatch => "vault_mismatch",
            Self::AmountMismatch => "amount_mismatch",
            Self::InvalidFeeConfig => "invalid_fee_config",
            Self::BelowCandidateThreshold => "below_candidate_threshold",
        }
    }

    pub fn stage(self) -> &'static str {
        match self {
            Self::Included | Self::BelowCandidateThreshold => "build_decoded",
            Self::TxFailed => "fetch_transactions",
            Self::NoCpmmSwap | Self::UnsupportedSwapInstruction | Self::DecodeFailed => {
                "decode_swaps"
            }
            Self::MissingPreState
            | Self::MissingTokenBalances
            | Self::MultiSwapAmbiguous
            | Self::VaultMismatch
            | Self::AmountMismatch
            | Self::InvalidFeeConfig => "prestate",
        }
    }
}
