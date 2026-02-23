use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported display currencies. Prices are always stored internally in curds (CURD).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Currency {
    #[default]
    Curds,
    Sats,
    Cents,
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Currency::Curds => write!(f, "CURD"),
            Currency::Sats => write!(f, "sats"),
            Currency::Cents => write!(f, "¢"),
        }
    }
}

/// Placeholder rate: 1 BTC ≈ $150,000 AUD → 1 sat = 0.0015 AUD = 0.15 cents.
/// So: AUD cents = curds * 0.15, AUD dollars = curds * 0.0015.
const SATS_TO_AUD: f64 = 0.0015;

/// Format an amount (stored in curds) for display in the given currency.
pub fn format_amount(amount_curds: u64, currency: &Currency) -> String {
    match currency {
        Currency::Curds => format!("{amount_curds} CURD"),
        Currency::Sats => format!("{amount_curds} sats"),
        Currency::Cents => {
            let aud = amount_curds as f64 * SATS_TO_AUD;
            format!("${aud:.2} AUD*")
        }
    }
}

impl Currency {
    pub fn all() -> &'static [Currency] {
        &[Currency::Curds, Currency::Sats, Currency::Cents]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Currency::Curds => "Curds",
            Currency::Sats => "Sats",
            Currency::Cents => "AUD",
        }
    }
}
