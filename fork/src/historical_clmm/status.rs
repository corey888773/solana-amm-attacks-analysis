use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    Included,
    TxFailed,
    NoClmmSwap,
    UnsupportedSwapInstruction,
    DecodeFailed,
    MissingTokenBalances,
    MultiSwapAmbiguous,
    AmountMismatch,
    MissingHistoricalState,
}

impl AnalysisStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Included => "included",
            Self::TxFailed => "tx_failed",
            Self::NoClmmSwap => "no_clmm_swap",
            Self::UnsupportedSwapInstruction => "unsupported_swap_instruction",
            Self::DecodeFailed => "decode_failed",
            Self::MissingTokenBalances => "missing_token_balances",
            Self::MultiSwapAmbiguous => "multi_swap_ambiguous",
            Self::AmountMismatch => "amount_mismatch",
            Self::MissingHistoricalState => "missing_historical_state",
        }
    }

    pub fn stage(self) -> &'static str {
        match self {
            Self::Included
            | Self::MissingTokenBalances
            | Self::AmountMismatch
            | Self::MissingHistoricalState => "build_decoded",
            Self::TxFailed => "fetch_transactions",
            Self::NoClmmSwap | Self::UnsupportedSwapInstruction | Self::DecodeFailed => {
                "decode_swaps"
            }
            Self::MultiSwapAmbiguous => "prestate",
        }
    }
}
