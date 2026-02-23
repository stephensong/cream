use chrono::{DateTime, Utc};
#[cfg(not(feature = "dev"))]
use ed25519_dalek::Verifier;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::identity::CustomerId;

/// State for a user's contract on the Freenet network.
///
/// Every transacting user (customer or supplier) gets a user contract that
/// provides persistent network presence: identity, supplier affiliation,
/// and a mock wallet balance checkpoint.
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
    /// Mock wallet balance (network-persistent checkpoint).
    pub balance_curds: u64,
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

    /// Merge another state into this one using LWW by `updated_at`.
    /// The `origin_supplier` field is immutable — once set, it cannot change.
    pub fn merge(&mut self, other: UserContractState) {
        if other.updated_at > self.updated_at {
            let preserved_origin = self.origin_supplier.clone();
            *self = other;
            // Preserve origin_supplier if it was already set
            if !preserved_origin.is_empty() {
                self.origin_supplier = preserved_origin;
            }
        }
    }

    /// Produce a summary for the delta sync protocol.
    pub fn summarize(&self) -> UserContractSummary {
        UserContractSummary {
            updated_at: Some(self.updated_at),
        }
    }

    /// Compute delta: return self if newer than the summary, or None-equivalent empty state.
    pub fn delta(&self, summary: &UserContractSummary) -> Option<UserContractState> {
        match summary.updated_at {
            Some(ts) if self.updated_at <= ts => None,
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
        newer.balance_curds = 9_000;
        state.merge(newer);
        assert_eq!(state.current_supplier, "Emma");
        assert_eq!(state.balance_curds, 9_000);
    }

    #[test]
    fn merge_preserves_origin_supplier() {
        let t1 = Utc::now() - chrono::Duration::hours(1);
        let t2 = Utc::now();
        let mut state = dummy_state(t1);
        let mut newer = dummy_state(t2);
        newer.origin_supplier = "Tampered".into();
        state.merge(newer);
        // origin_supplier should be preserved from the original
        assert_eq!(state.origin_supplier, "Gary");
    }

    #[test]
    fn merge_older_ignored() {
        let t1 = Utc::now();
        let t2 = Utc::now() - chrono::Duration::hours(1);
        let mut state = dummy_state(t1);
        state.balance_curds = 5_000;
        let older = dummy_state(t2);
        state.merge(older);
        assert_eq!(state.balance_curds, 5_000);
    }

    #[test]
    fn delta_returns_some_when_newer() {
        let state = dummy_state(Utc::now());
        let summary = UserContractSummary { updated_at: None };
        assert!(state.delta(&summary).is_some());
    }

    #[test]
    fn delta_returns_none_when_up_to_date() {
        let now = Utc::now();
        let state = dummy_state(now);
        let summary = UserContractSummary {
            updated_at: Some(now),
        };
        assert!(state.delta(&summary).is_none());
    }
}
