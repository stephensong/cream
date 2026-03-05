use serde::{Deserialize, Serialize};

/// A single wallet transaction (credit or debit) in the on-network ledger.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WalletTransaction {
    pub id: u32,
    pub kind: TransactionKind,
    pub amount: u64,
    pub description: String,
    pub sender: String,
    pub receiver: String,
    /// Shared reference linking this entry to the counterparty's matching entry.
    /// Format: "{sender}:{timestamp_millis}:{random}"
    pub tx_ref: String,
    pub timestamp: String,
    /// Lightning payment hash for peg-in/peg-out transactions.
    /// Used for contract-level deduplication to prevent double-minting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lightning_payment_hash: Option<String>,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionKind {
    Credit,
    Debit,
}
