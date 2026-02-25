use std::collections::HashSet;

use chrono::{DateTime, Utc};
#[cfg(not(feature = "dev"))]
use ed25519_dalek::Verifier;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::identity::CustomerId;
use crate::wallet::{TransactionKind, WalletTransaction};

/// State for a user's contract on the Freenet network.
///
/// Every transacting user (customer or supplier) gets a user contract that
/// provides persistent network presence: identity, supplier affiliation,
/// and an on-network transaction ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserContractState {
    /// The user's ed25519 public key (owner identity).
    pub owner: CustomerId,
    /// Display name / moniker.
    pub name: String,
    /// Supplier who onboarded this user (immutable after first set).
    pub origin_supplier: String,
    /// Supplier the user is currently connected to.
    pub current_supplier: String,
    /// Wallet balance — derived cache from ledger for quick reads.
    pub balance_curds: u64,
    /// User tree parent — who invited/onboarded this user.
    #[serde(default)]
    pub invited_by: String,
    /// On-network transaction ledger (append-only).
    #[serde(default)]
    pub ledger: Vec<WalletTransaction>,
    /// Monotonic transaction ID counter.
    #[serde(default)]
    pub next_tx_id: u32,
    /// Timestamp for LWW merge.
    pub updated_at: DateTime<Utc>,
    /// Owner's signature over the state.
    pub signature: Signature,
}

/// Parameters that make each user contract unique (same pattern as StorefrontParameters).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserContractParameters {
    pub owner: VerifyingKey,
}

/// Summary for delta sync protocol.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserContractSummary {
    pub updated_at: Option<DateTime<Utc>>,
    /// Number of ledger entries the summarizer already has.
    #[serde(default)]
    pub ledger_len: usize,
}

impl UserContractState {
    /// Serialize the state fields (excluding signature) for signing/verification.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let signable = SignableUserContract {
            owner: &self.owner,
            name: &self.name,
            origin_supplier: &self.origin_supplier,
            current_supplier: &self.current_supplier,
            balance_curds: self.balance_curds,
            invited_by: &self.invited_by,
            ledger_len: self.ledger.len(),
            updated_at: &self.updated_at,
        };
        serde_json::to_vec(&signable).expect("serialization should not fail")
    }

    /// Validate that the state is signed by the owner.
    pub fn validate(&self, owner: &VerifyingKey) -> bool {
        #[cfg(feature = "dev")]
        {
            let _ = owner;
            #[allow(clippy::needless_return)]
            return true;
        }
        #[cfg(not(feature = "dev"))]
        {
            let msg = self.signable_bytes();
            owner.verify(&msg, &self.signature).is_ok()
        }
    }

    /// Derive balance from the transaction ledger.
    pub fn derive_balance(&self) -> u64 {
        self.ledger.iter().fold(0u64, |acc, tx| match tx.kind {
            TransactionKind::Credit => acc.saturating_add(tx.amount),
            TransactionKind::Debit => acc.saturating_sub(tx.amount),
        })
    }

    /// Merge another state into this one.
    ///
    /// Hybrid strategy:
    /// - `invited_by`, `origin_supplier`: immutable (preserve if already set)
    /// - `current_supplier`, `updated_at`, `signature`: LWW (newer wins)
    /// - `ledger`: append-only union (dedup by tx_ref + kind)
    /// - `balance_curds`: re-derived from merged ledger
    pub fn merge(&mut self, other: UserContractState) {
        // Append-only ledger union (dedup by tx_ref + kind)
        let existing_keys: HashSet<(String, TransactionKind)> = self
            .ledger
            .iter()
            .map(|tx| (tx.tx_ref.clone(), tx.kind.clone()))
            .collect();
        for tx in other.ledger {
            let key = (tx.tx_ref.clone(), tx.kind.clone());
            if !existing_keys.contains(&key) {
                self.ledger.push(tx);
            }
        }
        // Sort by timestamp for display consistency
        self.ledger.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // LWW for metadata fields
        if other.updated_at > self.updated_at {
            let preserved_origin = self.origin_supplier.clone();
            let preserved_invited = self.invited_by.clone();
            let preserved_ledger = std::mem::take(&mut self.ledger);

            self.owner = other.owner;
            self.name = other.name;
            self.current_supplier = other.current_supplier;
            self.updated_at = other.updated_at;
            self.signature = other.signature;

            self.ledger = preserved_ledger;

            // Preserve immutable fields if already set
            if !preserved_origin.is_empty() {
                self.origin_supplier = preserved_origin;
            } else {
                self.origin_supplier = other.origin_supplier;
            }
            if !preserved_invited.is_empty() {
                self.invited_by = preserved_invited;
            } else {
                self.invited_by = other.invited_by;
            }
        }

        // Re-derive balance and next_tx_id from merged ledger
        self.balance_curds = self.derive_balance();
        self.next_tx_id = self.ledger.iter().map(|tx| tx.id).max().unwrap_or(0) + 1;
    }

    /// Produce a summary for the delta sync protocol.
    pub fn summarize(&self) -> UserContractSummary {
        UserContractSummary {
            updated_at: Some(self.updated_at),
            ledger_len: self.ledger.len(),
        }
    }

    /// Compute delta: return self if newer than the summary, or None-equivalent empty state.
    pub fn delta(&self, summary: &UserContractSummary) -> Option<UserContractState> {
        match summary.updated_at {
            Some(ts) if self.updated_at <= ts && self.ledger.len() <= summary.ledger_len => None,
            _ => Some(self.clone()),
        }
    }
}

#[derive(Serialize)]
struct SignableUserContract<'a> {
    owner: &'a CustomerId,
    name: &'a str,
    origin_supplier: &'a str,
    current_supplier: &'a str,
    balance_curds: u64,
    invited_by: &'a str,
    ledger_len: usize,
    updated_at: &'a DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn dummy_state(updated_at: DateTime<Utc>) -> UserContractState {
        let key = SigningKey::from_bytes(&[3u8; 32]);
        UserContractState {
            owner: CustomerId(key.verifying_key()),
            name: "Alice".into(),
            origin_supplier: "Gary".into(),
            current_supplier: "Gary".into(),
            balance_curds: 10_000,
            invited_by: "Gary".into(),
            ledger: vec![WalletTransaction {
                id: 0,
                kind: TransactionKind::Credit,
                amount: 10_000,
                description: "Initial CURD allocation".into(),
                sender: "__cream_root__".into(),
                receiver: "Alice".into(),
                tx_ref: "root:1000:42".into(),
                timestamp: "2026-01-01T00:00:00.000Z".into(),
            }],
            next_tx_id: 1,
            updated_at,
            signature: Signature::from_bytes(&[0u8; 64]),
        }
    }

    #[test]
    fn merge_lww_newer_wins() {
        let t1 = Utc::now() - chrono::Duration::hours(1);
        let t2 = Utc::now();
        let mut state = dummy_state(t1);
        let mut newer = dummy_state(t2);
        newer.current_supplier = "Emma".into();
        state.merge(newer);
        assert_eq!(state.current_supplier, "Emma");
        // Balance re-derived from ledger (10_000 credit)
        assert_eq!(state.balance_curds, 10_000);
    }

    #[test]
    fn merge_preserves_origin_supplier() {
        let t1 = Utc::now() - chrono::Duration::hours(1);
        let t2 = Utc::now();
        let mut state = dummy_state(t1);
        let mut newer = dummy_state(t2);
        newer.origin_supplier = "Tampered".into();
        state.merge(newer);
        assert_eq!(state.origin_supplier, "Gary");
    }

    #[test]
    fn merge_preserves_invited_by() {
        let t1 = Utc::now() - chrono::Duration::hours(1);
        let t2 = Utc::now();
        let mut state = dummy_state(t1);
        let mut newer = dummy_state(t2);
        newer.invited_by = "Tampered".into();
        state.merge(newer);
        assert_eq!(state.invited_by, "Gary");
    }

    #[test]
    fn merge_older_ignored_but_ledger_union() {
        let t1 = Utc::now();
        let t2 = Utc::now() - chrono::Duration::hours(1);
        let mut state = dummy_state(t1);
        let mut older = dummy_state(t2);
        // Add a new transaction to the older state
        older.ledger.push(WalletTransaction {
            id: 1,
            kind: TransactionKind::Debit,
            amount: 100,
            description: "Toll".into(),
            sender: "Alice".into(),
            receiver: "__cream_root__".into(),
            tx_ref: "alice:2000:99".into(),
            timestamp: "2026-01-01T00:01:00.000Z".into(),
        });
        state.merge(older);
        // Metadata unchanged (older timestamp)
        assert_eq!(state.current_supplier, "Gary");
        // But ledger got the new entry
        assert_eq!(state.ledger.len(), 2);
        assert_eq!(state.balance_curds, 9_900);
    }

    #[test]
    fn merge_ledger_dedup() {
        let t1 = Utc::now() - chrono::Duration::hours(1);
        let t2 = Utc::now();
        let mut state = dummy_state(t1);
        let other = dummy_state(t2);
        // Both have the same initial transaction (same tx_ref + kind)
        state.merge(other);
        assert_eq!(state.ledger.len(), 1, "duplicate tx should not be added");
    }

    #[test]
    fn derive_balance_from_ledger() {
        let mut state = dummy_state(Utc::now());
        assert_eq!(state.derive_balance(), 10_000);
        state.ledger.push(WalletTransaction {
            id: 1,
            kind: TransactionKind::Debit,
            amount: 500,
            description: "Test".into(),
            sender: "Alice".into(),
            receiver: "Bob".into(),
            tx_ref: "test:1:1".into(),
            timestamp: "2026-01-02T00:00:00.000Z".into(),
        });
        assert_eq!(state.derive_balance(), 9_500);
    }

    #[test]
    fn delta_returns_some_when_newer() {
        let state = dummy_state(Utc::now());
        let summary = UserContractSummary {
            updated_at: None,
            ledger_len: 0,
        };
        assert!(state.delta(&summary).is_some());
    }

    #[test]
    fn delta_returns_none_when_up_to_date() {
        let now = Utc::now();
        let state = dummy_state(now);
        let summary = UserContractSummary {
            updated_at: Some(now),
            ledger_len: 1,
        };
        assert!(state.delta(&summary).is_none());
    }

    #[test]
    fn delta_returns_some_when_ledger_newer() {
        let now = Utc::now();
        let state = dummy_state(now);
        let summary = UserContractSummary {
            updated_at: Some(now),
            ledger_len: 0, // summary has fewer ledger entries
        };
        assert!(state.delta(&summary).is_some());
    }
}
