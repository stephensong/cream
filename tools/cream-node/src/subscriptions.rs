use dashmap::DashMap;
use tokio::sync::broadcast;

/// Manages per-contract subscription channels.
///
/// Each contract key maps to a broadcast::Sender that fans out
/// serialized UpdateNotification bytes to all subscribers.
pub struct SubscriptionManager {
    channels: DashMap<String, broadcast::Sender<Vec<u8>>>,
}

impl SubscriptionManager {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }

    /// Subscribe to updates for a contract. Returns a receiver that will
    /// get serialized HostResult bytes for each UpdateNotification.
    pub fn subscribe(&self, instance_id: &str) -> broadcast::Receiver<Vec<u8>> {
        let entry = self
            .channels
            .entry(instance_id.to_string())
            .or_insert_with(|| broadcast::channel(64).0);
        entry.subscribe()
    }

    /// Broadcast an update notification to all subscribers of a contract.
    /// The `notification_bytes` should be a bincode-serialized `Ok(HostResponse::ContractResponse(UpdateNotification))`.
    pub fn notify(&self, instance_id: &str, notification_bytes: Vec<u8>) {
        if let Some(sender) = self.channels.get(instance_id) {
            // Ignore send errors (no active receivers)
            let _ = sender.send(notification_bytes);
        }
    }
}
