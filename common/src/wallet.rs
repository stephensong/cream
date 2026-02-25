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
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionKind {
    Credit,
    Debit,
}
