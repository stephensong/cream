use serde::{Deserialize, Serialize};

/// Receipt returned after a successful wallet operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferReceipt {
    /// Shared reference linking debit/credit entries (e.g. "alice:1700000000000:abc123").
    pub tx_ref: String,
    /// Amount transferred in CURD.
    pub amount: u64,
    /// ISO 8601 timestamp of the transfer.
    pub timestamp: String,
    /// Bearer token (OOBNotes) for Fedimint backend; None for CREAM-native.
    pub bearer_token: Option<String>,
}

/// Errors from wallet operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WalletError {
    InsufficientBalance { available: u64, requested: u64 },
    TransferFailed(String),
    BackendUnavailable(String),
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientBalance {
                available,
                requested,
            } => write!(
                f,
                "insufficient balance: have {available}, need {requested}"
            ),
            Self::TransferFailed(msg) => write!(f, "transfer failed: {msg}"),
            Self::BackendUnavailable(msg) => write!(f, "backend unavailable: {msg}"),
        }
    }
}

/// Abstraction over wallet backends (CREAM-native ledger vs Fedimint e-cash).
///
/// Escrow methods map to the order lifecycle:
/// - `PlaceOrder`   → `escrow_lock`    (customer locks deposit)
/// - `FulfillOrder`  → `escrow_release` (supplier receives deposit)
/// - `CancelOrder`   → `escrow_cancel`  (customer gets refund)
#[allow(async_fn_in_trait)]
pub trait WalletBackend {
    /// Current balance in CURD.
    async fn balance(&self) -> Result<u64, WalletError>;

    /// Transfer funds to a recipient.
    async fn transfer(
        &mut self,
        amount: u64,
        description: String,
        recipient: String,
    ) -> Result<TransferReceipt, WalletError>;

    /// Receive funds from a bearer token (Fedimint OOBNotes) or incoming transfer reference.
    async fn receive(&mut self, token: &str) -> Result<u64, WalletError>;

    /// Lock funds in escrow. Returns a token string that can be released or cancelled.
    async fn escrow_lock(
        &mut self,
        amount: u64,
        description: String,
    ) -> Result<String, WalletError>;

    /// Release escrowed funds to a recipient.
    async fn escrow_release(
        &mut self,
        token: &str,
        recipient: String,
    ) -> Result<TransferReceipt, WalletError>;

    /// Cancel an escrow, returning funds to the original owner.
    async fn escrow_cancel(&mut self, token: &str) -> Result<u64, WalletError>;

    /// Human-readable backend name (e.g. "cream-native", "fedimint").
    fn backend_name(&self) -> &str;
}
