use std::collections::HashMap;

use dioxus::prelude::*;

use cream_common::directory::{DirectoryEntry, DirectoryState};
use cream_common::storefront::StorefrontState;

/// Network-sourced state shared across all components.
///
/// Updated reactively when Freenet contract notifications arrive.
/// Components read from this for cross-user data (directory, storefronts).
#[derive(Clone, Debug, Default)]
pub struct SharedState {
    /// Supplier directory from the directory contract.
    pub directory: DirectoryState,
    /// Subscribed storefronts keyed by supplier name.
    pub storefronts: HashMap<String, StorefrontState>,
    /// Whether we're connected to a Freenet node.
    pub connected: bool,
    /// Last error message from node communication.
    pub last_error: Option<String>,
}

impl SharedState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all directory entries sorted by name.
    pub fn supplier_entries(&self) -> Vec<&DirectoryEntry> {
        let mut entries: Vec<_> = self.directory.entries.values().collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }
}

pub fn use_shared_state() -> Signal<SharedState> {
    use_context::<Signal<SharedState>>()
}
