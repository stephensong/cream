use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for a message (random u64, same pattern as OrderId).
pub type MessageId = u64;

/// A message sent within a supplier's storefront.
///
/// Messages live inside the StorefrontState and are append-only.
/// Each message costs a toll (burned from the sender's wallet) to prevent spam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub sender_name: String,
    pub sender_key: Option<String>,
    pub body: String,
    pub toll_paid: u64,
    pub created_at: DateTime<Utc>,
    pub reply_to: Option<MessageId>,
}
