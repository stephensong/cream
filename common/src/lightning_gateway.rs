use serde::{Deserialize, Serialize};

/// Exchange rate: 1 satoshi = 10 CURD.
pub const CURD_PER_SAT: u64 = 10;

/// Status of a Lightning payment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentStatus {
    Success { preimage: String },
    Failed { reason: String },
    Pending,
}

/// A Lightning Network invoice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LnInvoice {
    pub bolt11: String,
    pub amount_sats: u64,
    pub memo: String,
    pub payment_hash: String,
}

/// Errors from gateway operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GatewayError {
    PaymentFailed(String),
    Unavailable(String),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PaymentFailed(msg) => write!(f, "payment failed: {msg}"),
            Self::Unavailable(msg) => write!(f, "gateway unavailable: {msg}"),
        }
    }
}

/// Abstraction over Lightning Network backends (LND, mock, etc.).
///
/// Guardian nodes will run LND directly (auto-wired by StartOS).
/// For development, a mock implementation provides instant settlement.
pub trait LightningGateway {
    /// Create a new invoice for receiving payment.
    fn create_invoice(&mut self, amount_sats: u64, memo: &str) -> Result<LnInvoice, GatewayError>;

    /// Pay an existing BOLT11 invoice.
    fn pay_invoice(&mut self, bolt11: &str) -> Result<PaymentStatus, GatewayError>;

    /// Check the status of a previously created invoice.
    fn check_invoice(&self, payment_hash: &str) -> Result<PaymentStatus, GatewayError>;

    /// Human-readable name of this gateway backend.
    fn gateway_name(&self) -> &str;
}
