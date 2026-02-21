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
    /// Map from supplier name to their storefront contract key (as string).
    pub storefront_keys: HashMap<String, String>,
    /// Whether we're connected to a Freenet node.
    pub connected: bool,
    /// Directory contract key (set after PUT or GET).
    pub directory_contract_key: Option<String>,
    /// Last error message from node communication.
    pub last_error: Option<String>,
}

impl SharedState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all directory entries sorted by name.
    #[allow(dead_code)] // useful utility, will be used
    pub fn supplier_entries(&self) -> Vec<&DirectoryEntry> {
        let mut entries: Vec<_> = self.directory.entries.values().collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }
}

pub fn use_shared_state() -> Signal<SharedState> {
    use_context::<Signal<SharedState>>()
}
