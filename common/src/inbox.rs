use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use crate::identity::UserId;

/// Unique identifier for an inbox message (random u64).
pub type MessageId = u64;

/// The kind of inbox message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MessageKind {
    /// A direct text message.
    DirectMessage,
    /// A chat invite with a session ID for the relay.
    ChatInvite { session_id: String },
    /// Market organizer invites a supplier to participate.
    MarketInvite { market_name: String },
    /// Supplier accepts a market invitation.
    MarketAccept { market_name: String },
}

/// A message delivered to a user's inbox contract.
///
/// Messages are append-only and pruned after 30 days.
/// Each message costs a toll (burned from the sender's wallet) to prevent spam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxMessage {
    pub id: MessageId,
    pub kind: MessageKind,
    pub from_name: String,
    /// Sender's user contract key (Base58), if known.
    pub from_key: Option<String>,
    pub body: String,
    pub toll_paid: u64,
    pub created_at: DateTime<Utc>,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The full inbox state stored in a per-user Freenet contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxState {
    pub owner: UserId,
    pub messages: BTreeMap<MessageId, InboxMessage>,
    pub updated_at: DateTime<Utc>,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Parameters that make each inbox contract unique.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxParameters {
    pub owner: VerifyingKey,
}

/// Summary for delta sync protocol.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InboxSummary {
    pub message_ids: BTreeSet<MessageId>,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl InboxState {
    /// Remove messages older than 30 days.
    /// Returns `true` if any messages were removed.
    pub fn prune_old_messages(&mut self, now: DateTime<Utc>) -> bool {
        let cutoff = now - chrono::Duration::days(30);
        let before = self.messages.len();
        self.messages.retain(|_, msg| msg.created_at >= cutoff);
        self.messages.len() != before
    }

    /// Merge another inbox state into this one (union-append by MessageId).
    pub fn merge(&mut self, other: InboxState) {
        for (id, message) in other.messages {
            self.messages.entry(id).or_insert(message);
        }
        if other.updated_at > self.updated_at {
            self.updated_at = other.updated_at;
        }
    }

    /// Validate an update: only additions are accepted (no removals or edits).
    pub fn validate_update(&self, update: &InboxState) -> bool {
        // Update must have the same owner
        if self.owner != update.owner {
            return false;
        }
        // All messages in the update must either be new or identical to existing
        for (id, msg) in &update.messages {
            if let Some(existing) = self.messages.get(id) {
                if existing != msg {
                    return false; // can't modify existing messages
                }
            }
        }
        true
    }

    /// Validate full state (owner check).
    pub fn validate(&self, _owner: &VerifyingKey) -> bool {
        // Inbox messages don't require owner signature — anyone can append.
        // The toll payment provides spam control.
        true
    }

    /// Summarize: return set of known message IDs.
    pub fn summarize(&self) -> InboxSummary {
        InboxSummary {
            message_ids: self.messages.keys().cloned().collect(),
            extra: Default::default(),
        }
    }

    /// Compute delta: messages not in the summary.
    pub fn delta(&self, summary: &InboxSummary) -> Option<InboxState> {
        let new_messages: BTreeMap<MessageId, InboxMessage> = self
            .messages
            .iter()
            .filter(|(id, _)| !summary.message_ids.contains(id))
            .map(|(id, m)| (*id, m.clone()))
            .collect();

        if new_messages.is_empty() {
            return None;
        }

        Some(InboxState {
            owner: self.owner.clone(),
            messages: new_messages,
            updated_at: self.updated_at,
            extra: Default::default(),
        })
    }
}
