use std::collections::HashSet;

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::identity::UserId;
use crate::tolls::TollRates;
use crate::wallet::{TransactionKind, WalletTransaction};

/// Prune when ledger exceeds this many entries.
pub const PRUNE_THRESHOLD: usize = 500;
/// Keep this many recent entries after pruning.
pub const PRUNE_KEEP_RECENT: usize = 50;

/// State for a user's contract on the Freenet network.
///
/// Every transacting user (customer or supplier) gets a user contract that
/// provides persistent network presence: identity, supplier affiliation,
/// and an on-network transaction ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserContractState {
    /// The user's ed25519 public key (owner identity).
    pub owner: UserId,
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
    /// Guardian-configurable toll rates (only meaningful on root contract).
    #[serde(default)]
    pub toll_rates: TollRates,
    /// Balance at the time of the last checkpoint (pruned txs folded into this).
    #[serde(default)]
    pub checkpoint_balance: u64,
    /// Number of transactions included in the checkpoint (pruned prefix count).
    #[serde(default)]
    pub checkpoint_tx_count: u64,
    /// Timestamp of the last checkpoint.
    #[serde(default)]
    pub checkpoint_at: Option<DateTime<Utc>>,
    /// Lightning payment hashes from pruned transactions (preserved for double-mint prevention).
    #[serde(default)]
    pub pruned_lightning_hashes: HashSet<String>,
    /// Timestamp for LWW merge.
    pub updated_at: DateTime<Utc>,
    /// Owner's signature over the state.
    pub signature: Signature,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    /// Number of pruned transactions (checkpoint prefix count).
    #[serde(default)]
    pub checkpoint_tx_count: u64,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
            toll_rates: &self.toll_rates,
            checkpoint_balance: self.checkpoint_balance,
            checkpoint_tx_count: self.checkpoint_tx_count,
            checkpoint_at: &self.checkpoint_at,
            updated_at: &self.updated_at,
        };
        serde_json::to_vec(&signable).expect("serialization should not fail")
    }

    /// Validate that the state is signed by the owner.
    pub fn validate(&self, owner: &VerifyingKey) -> bool {
        #[cfg(feature = "dev")]
        {
            // In dev mode, skip signature verification entirely.
            // DKG produces a different group key than dev_root_frost_keys(),
            // so root contract signatures from the live guardians won't match
            // the trusted-dealer key embedded in the contract parameters.
            let _ = owner;
            return true;
        }
        #[cfg(not(feature = "dev"))]
        {
            let msg = self.signable_bytes();
            owner.verify(&msg, &self.signature).is_ok()
        }
    }

    /// Validate an incoming update for merge.
    ///
    /// - If the update only adds Credit entries (no debits, no metadata changes), accept without signature check
    /// - If the update contains Debit entries or metadata changes, require owner signature
    pub fn validate_update(&self, update: &UserContractState, owner: &VerifyingKey) -> bool {
        #[cfg(feature = "dev")]
        {
            // In dev mode, skip signature verification (DKG key ≠ trusted-dealer key).
            let _ = update;
            let _ = owner;
            #[allow(clippy::needless_return)]
            return true;
        }
        #[cfg(not(feature = "dev"))]
        {
            self.validate_update_inner(update, owner)
        }
    }

    /// Inner validation logic shared by dev (root-only) and production paths.
    fn validate_update_inner(&self, update: &UserContractState, owner: &VerifyingKey) -> bool {
        // Find new ledger entries (not already in self)
        let existing_keys: HashSet<(String, TransactionKind)> = self
            .ledger
            .iter()
            .map(|tx| (tx.tx_ref.clone(), tx.kind.clone()))
            .collect();

        let new_entries: Vec<&WalletTransaction> = update
            .ledger
            .iter()
            .filter(|tx| !existing_keys.contains(&(tx.tx_ref.clone(), tx.kind.clone())))
            .collect();

        // Check if metadata changed (includes checkpoint changes)
        let metadata_changed = update.name != self.name
            || update.origin_supplier != self.origin_supplier
            || update.current_supplier != self.current_supplier
            || update.invited_by != self.invited_by
            || update.owner != self.owner
            || update.toll_rates != self.toll_rates
            || update.checkpoint_balance != self.checkpoint_balance
            || update.checkpoint_tx_count != self.checkpoint_tx_count
            || update.checkpoint_at != self.checkpoint_at;

        // If all new entries are credits and no metadata changed, accept without sig
        let all_credits = new_entries
            .iter()
            .all(|tx| tx.kind == TransactionKind::Credit);
        let has_debits = new_entries
            .iter()
            .any(|tx| tx.kind == TransactionKind::Debit);

        if !has_debits && all_credits && !metadata_changed {
            return true;
        }

        // Otherwise require owner signature
        update.validate(owner)
    }

    /// Derive balance from the transaction ledger, starting from checkpoint_balance.
    pub fn derive_balance(&self) -> u64 {
        self.ledger
            .iter()
            .fold(self.checkpoint_balance, |acc, tx| match tx.kind {
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
        // Checkpoint LWW: newer checkpoint_at wins
        let other_cp_at = other.checkpoint_at;
        match (self.checkpoint_at, other_cp_at) {
            (Some(mine), Some(theirs)) if theirs > mine => {
                self.checkpoint_balance = other.checkpoint_balance;
                self.checkpoint_tx_count = other.checkpoint_tx_count;
                self.checkpoint_at = other_cp_at;
            }
            (None, Some(_)) => {
                self.checkpoint_balance = other.checkpoint_balance;
                self.checkpoint_tx_count = other.checkpoint_tx_count;
                self.checkpoint_at = other_cp_at;
            }
            _ => {}
        }
        // Pruned lightning hashes: set union (always merge both sides)
        for hash in &other.pruned_lightning_hashes {
            self.pruned_lightning_hashes.insert(hash.clone());
        }

        // Append-only ledger union (dedup by tx_ref + kind)
        let existing_keys: HashSet<(String, TransactionKind)> = self
            .ledger
            .iter()
            .map(|tx| (tx.tx_ref.clone(), tx.kind.clone()))
            .collect();

        // Collect existing Lightning payment hashes (live + pruned) to prevent double-minting
        let existing_ln_hashes: HashSet<String> = self
            .ledger
            .iter()
            .filter_map(|tx| tx.lightning_payment_hash.clone())
            .chain(self.pruned_lightning_hashes.iter().cloned())
            .collect();

        for tx in other.ledger {
            let key = (tx.tx_ref.clone(), tx.kind.clone());
            if existing_keys.contains(&key) {
                continue;
            }
            // Reject transactions with a lightning_payment_hash already in the ledger or pruned set
            if let Some(ref hash) = tx.lightning_payment_hash {
                if existing_ln_hashes.contains(hash) {
                    continue;
                }
            }
            self.ledger.push(tx);
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
            self.toll_rates = other.toll_rates;
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
            checkpoint_tx_count: self.checkpoint_tx_count,
            extra: Default::default(),
        }
    }

    /// Compute delta: return self if newer than the summary, or None-equivalent empty state.
    pub fn delta(&self, summary: &UserContractSummary) -> Option<UserContractState> {
        match summary.updated_at {
            Some(ts)
                if self.updated_at <= ts
                    && self.ledger.len() <= summary.ledger_len
                    && self.checkpoint_tx_count <= summary.checkpoint_tx_count =>
            {
                None
            }
            _ => Some(self.clone()),
        }
    }

    /// Perform a checkpoint: fold current ledger into checkpoint_balance and prune old entries.
    ///
    /// Keeps the last `keep_recent` entries for display. Returns the number of pruned entries.
    pub fn checkpoint(&mut self, keep_recent: usize) -> usize {
        if self.ledger.is_empty() {
            return 0;
        }

        let new_balance = self.derive_balance();

        // Extract lightning hashes from entries that will be pruned
        let prune_count = self.ledger.len().saturating_sub(keep_recent);
        for tx in self.ledger.iter().take(prune_count) {
            if let Some(ref hash) = tx.lightning_payment_hash {
                self.pruned_lightning_hashes.insert(hash.clone());
            }
        }

        // Keep only the last `keep_recent` entries
        if prune_count > 0 {
            self.ledger = self.ledger.split_off(prune_count);
        }

        // Update checkpoint fields: the checkpoint_balance now covers everything
        // up to (but not including) the remaining ledger entries.
        // So checkpoint_balance = new_balance - contribution_of_remaining_entries
        let remaining_contribution =
            self.ledger
                .iter()
                .fold(0i64, |acc, tx| match tx.kind {
                    TransactionKind::Credit => acc + tx.amount as i64,
                    TransactionKind::Debit => acc - tx.amount as i64,
                });
        self.checkpoint_balance = (new_balance as i64 - remaining_contribution) as u64;
        self.checkpoint_tx_count += prune_count as u64;
        self.checkpoint_at = Some(Utc::now());

        // Re-derive to ensure consistency
        self.balance_curds = self.derive_balance();

        prune_count
    }
}

#[derive(Serialize)]
struct SignableUserContract<'a> {
    owner: &'a UserId,
    name: &'a str,
    origin_supplier: &'a str,
    current_supplier: &'a str,
    balance_curds: u64,
    invited_by: &'a str,
    ledger_len: usize,
    toll_rates: &'a TollRates,
    checkpoint_balance: u64,
    checkpoint_tx_count: u64,
    checkpoint_at: &'a Option<DateTime<Utc>>,
    updated_at: &'a DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn dummy_state(updated_at: DateTime<Utc>) -> UserContractState {
        let key = SigningKey::from_bytes(&[3u8; 32]);
        UserContractState {
            owner: UserId(key.verifying_key()),
            name: "Alice".into(),
            origin_supplier: "Gary".into(),
            current_supplier: "Gary".into(),
            balance_curds: 10_000,
            invited_by: "Gary".into(),
            toll_rates: TollRates::default(),
            checkpoint_balance: 0,
            checkpoint_tx_count: 0,
            checkpoint_at: None,
            pruned_lightning_hashes: HashSet::new(),
            ledger: vec![WalletTransaction {
                id: 0,
                kind: TransactionKind::Credit,
                amount: 10_000,
                description: "Initial CURD allocation".into(),
                sender: "__cream_root__".into(),
                receiver: "Alice".into(),
                tx_ref: "root:1000:42".into(),
                timestamp: "2026-01-01T00:00:00.000Z".into(),
                lightning_payment_hash: None,
                extra: Default::default(),
            }],
            next_tx_id: 1,
            updated_at,
            signature: Signature::from_bytes(&[0u8; 64]),
            extra: Default::default(),
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
            lightning_payment_hash: None,
            extra: Default::default(),
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
            lightning_payment_hash: None,
            extra: Default::default(),
        });
        assert_eq!(state.derive_balance(), 9_500);
    }

    #[test]
    fn validate_update_credit_only_accepted_without_sig() {
        let state = dummy_state(Utc::now());
        let mut update = state.clone();
        // Add a credit entry to the update
        update.ledger.push(WalletTransaction {
            id: 1,
            kind: TransactionKind::Credit,
            amount: 500,
            description: "Third-party credit".into(),
            sender: "Bob".into(),
            receiver: "Alice".into(),
            tx_ref: "bob:1234:1".into(),
            timestamp: "2026-01-02T00:00:00.000Z".into(),
            lightning_payment_hash: None,
            extra: Default::default(),
        });
        update.signature = Signature::from_bytes(&[0u8; 64]); // invalid sig

        let key = SigningKey::from_bytes(&[3u8; 32]);
        // In dev mode this always passes; in prod mode credit-only should pass without valid sig
        assert!(
            state.validate_update(&update, &key.verifying_key()),
            "Credit-only update should be accepted without valid signature"
        );
    }

    #[cfg(not(feature = "dev"))]
    #[test]
    fn validate_update_debit_rejected_without_sig() {
        let state = dummy_state(Utc::now());
        let mut update = state.clone();
        // Add a debit entry to the update
        update.ledger.push(WalletTransaction {
            id: 1,
            kind: TransactionKind::Debit,
            amount: 500,
            description: "Unauthorized debit".into(),
            sender: "Alice".into(),
            receiver: "Eve".into(),
            tx_ref: "eve:1234:1".into(),
            timestamp: "2026-01-02T00:00:00.000Z".into(),
            lightning_payment_hash: None,
            extra: Default::default(),
        });
        update.signature = Signature::from_bytes(&[0u8; 64]); // invalid sig

        let key = SigningKey::from_bytes(&[3u8; 32]);
        assert!(
            !state.validate_update(&update, &key.verifying_key()),
            "Debit update should be rejected without valid signature"
        );
    }

    #[cfg(not(feature = "dev"))]
    #[test]
    fn validate_update_metadata_change_rejected_without_sig() {
        let state = dummy_state(Utc::now());
        let mut update = state.clone();
        update.current_supplier = "Hacker".into();
        update.signature = Signature::from_bytes(&[0u8; 64]); // invalid sig

        let key = SigningKey::from_bytes(&[3u8; 32]);
        assert!(
            !state.validate_update(&update, &key.verifying_key()),
            "Metadata-changing update should be rejected without valid signature"
        );
    }

    #[test]
    fn delta_returns_some_when_newer() {
        let state = dummy_state(Utc::now());
        let summary = UserContractSummary {
            updated_at: None,
            ledger_len: 0,
            checkpoint_tx_count: 0,
            extra: Default::default(),
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
            checkpoint_tx_count: 0,
            extra: Default::default(),
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
            checkpoint_tx_count: 0,
            extra: Default::default(),
        };
        assert!(state.delta(&summary).is_some());
    }

    #[test]
    fn delta_returns_some_when_checkpoint_newer() {
        let now = Utc::now();
        let mut state = dummy_state(now);
        state.checkpoint_tx_count = 5;
        let summary = UserContractSummary {
            updated_at: Some(now),
            ledger_len: 1,
            checkpoint_tx_count: 0,
            extra: Default::default(),
        };
        assert!(state.delta(&summary).is_some());
    }

    // ─── Checkpoint tests ───────────────────────────────────────────────────

    fn make_tx(id: u32, kind: TransactionKind, amount: u64, tx_ref: &str) -> WalletTransaction {
        WalletTransaction {
            id,
            kind,
            amount,
            description: "test".into(),
            sender: "Alice".into(),
            receiver: "Bob".into(),
            tx_ref: tx_ref.into(),
            timestamp: format!("2026-01-01T00:{:02}:00.000Z", id),
            lightning_payment_hash: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn checkpoint_preserves_balance() {
        let mut state = dummy_state(Utc::now());
        // Add more transactions
        for i in 1..10 {
            state.ledger.push(make_tx(
                i,
                TransactionKind::Credit,
                100,
                &format!("tx:{}", i),
            ));
        }
        state.balance_curds = state.derive_balance();
        let balance_before = state.balance_curds;
        assert_eq!(balance_before, 10_900); // 10_000 + 9*100

        state.checkpoint(3);

        assert_eq!(state.derive_balance(), balance_before);
        assert_eq!(state.balance_curds, balance_before);
        assert_eq!(state.ledger.len(), 3); // kept last 3
        assert_eq!(state.checkpoint_tx_count, 7); // pruned 7
        assert!(state.checkpoint_at.is_some());
    }

    #[test]
    fn checkpoint_empty_ledger_noop() {
        let mut state = dummy_state(Utc::now());
        state.ledger.clear();
        state.balance_curds = 0;
        let pruned = state.checkpoint(50);
        assert_eq!(pruned, 0);
        assert_eq!(state.checkpoint_tx_count, 0);
    }

    #[test]
    fn checkpoint_preserves_lightning_hashes() {
        let mut state = dummy_state(Utc::now());
        // Add a transaction with a lightning hash
        let mut ln_tx = make_tx(1, TransactionKind::Credit, 500, "ln:1");
        ln_tx.lightning_payment_hash = Some("abc123".into());
        state.ledger.push(ln_tx);
        state.balance_curds = state.derive_balance();

        state.checkpoint(0); // prune everything

        assert!(state.pruned_lightning_hashes.contains("abc123"));
        assert_eq!(state.ledger.len(), 0);
    }

    #[test]
    fn merge_with_checkpoint_lww() {
        let t1 = Utc::now() - chrono::Duration::hours(2);
        let t2 = Utc::now() - chrono::Duration::hours(1);
        let t3 = Utc::now();

        let mut state = dummy_state(t1);
        state.checkpoint_at = Some(t2);
        state.checkpoint_balance = 5_000;
        state.checkpoint_tx_count = 10;

        let mut other = dummy_state(t3);
        other.checkpoint_at = Some(t3);
        other.checkpoint_balance = 8_000;
        other.checkpoint_tx_count = 20;

        state.merge(other);

        // Newer checkpoint wins
        assert_eq!(state.checkpoint_balance, 8_000);
        assert_eq!(state.checkpoint_tx_count, 20);
        assert_eq!(state.checkpoint_at, Some(t3));
    }

    #[test]
    fn merge_pruned_lightning_hashes_union() {
        let t1 = Utc::now() - chrono::Duration::hours(1);
        let t2 = Utc::now();

        let mut state = dummy_state(t1);
        state.pruned_lightning_hashes.insert("hash_a".into());

        let mut other = dummy_state(t2);
        other.pruned_lightning_hashes.insert("hash_b".into());

        state.merge(other);

        assert!(state.pruned_lightning_hashes.contains("hash_a"));
        assert!(state.pruned_lightning_hashes.contains("hash_b"));
    }

    #[test]
    fn merge_rejects_duplicate_pruned_lightning_hash() {
        let t1 = Utc::now() - chrono::Duration::hours(1);
        let t2 = Utc::now();

        let mut state = dummy_state(t1);
        state.pruned_lightning_hashes.insert("used_hash".into());

        let mut other = dummy_state(t2);
        // Other has a new transaction with the same lightning hash
        let mut dup_tx = make_tx(5, TransactionKind::Credit, 999, "sneaky:1");
        dup_tx.lightning_payment_hash = Some("used_hash".into());
        other.ledger.push(dup_tx);

        state.merge(other);

        // The duplicate-hash tx should NOT have been added
        assert!(
            !state.ledger.iter().any(|tx| tx.tx_ref == "sneaky:1"),
            "Transaction with pruned lightning hash should be rejected"
        );
    }

    #[test]
    fn derive_balance_uses_checkpoint() {
        let mut state = dummy_state(Utc::now());
        state.checkpoint_balance = 5_000;
        // The existing ledger has a 10_000 credit
        assert_eq!(state.derive_balance(), 15_000);
    }

    #[test]
    fn double_checkpoint_accumulates() {
        let mut state = dummy_state(Utc::now());
        for i in 1..20 {
            state.ledger.push(make_tx(
                i,
                TransactionKind::Credit,
                100,
                &format!("tx:{}", i),
            ));
        }
        state.balance_curds = state.derive_balance();
        let expected_balance = state.derive_balance(); // 10_000 + 19*100 = 11_900

        // First checkpoint: keep 5
        state.checkpoint(5);
        assert_eq!(state.derive_balance(), expected_balance);
        assert_eq!(state.ledger.len(), 5);
        let first_cp_count = state.checkpoint_tx_count;
        assert_eq!(first_cp_count, 15);

        // Second checkpoint: keep 2
        state.checkpoint(2);
        assert_eq!(state.derive_balance(), expected_balance);
        assert_eq!(state.ledger.len(), 2);
        assert_eq!(state.checkpoint_tx_count, 18); // 15 + 3
    }
}
