use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
#[cfg(not(feature = "dev"))]
use ed25519_dalek::Verifier;
use freenet_stdlib::prelude::ContractKey;
use serde::{Deserialize, Serialize};

use crate::identity::SupplierId;
use crate::location::GeoLocation;
use crate::product::ProductCategory;

/// A single supplier's entry in the global directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub supplier: SupplierId,
    pub name: String,
    pub description: String,
    pub location: GeoLocation,
    pub categories: Vec<ProductCategory>,
    pub storefront_key: ContractKey,
    pub updated_at: DateTime<Utc>,
    pub signature: Signature,
}

impl DirectoryEntry {
    /// Serialize the signable fields (everything except signature).
    pub fn signable_bytes(&self) -> Vec<u8> {
        let signable = SignableDirectoryEntry {
            supplier: &self.supplier,
            name: &self.name,
            description: &self.description,
            location: &self.location,
            categories: &self.categories,
            storefront_key: &self.storefront_key,
            updated_at: &self.updated_at,
        };
        serde_json::to_vec(&signable).expect("serialization should not fail")
    }

    /// Verify that the entry was signed by the supplier's key.
    pub fn verify_signature(&self) -> bool {
        #[cfg(feature = "dev")]
        {
            #[allow(clippy::needless_return)]
            return true;
        }
        #[cfg(not(feature = "dev"))]
        {
            let msg = self.signable_bytes();
            self.supplier.0.verify(&msg, &self.signature).is_ok()
        }
    }
}

#[derive(Serialize)]
struct SignableDirectoryEntry<'a> {
    supplier: &'a SupplierId,
    name: &'a str,
    description: &'a str,
    location: &'a GeoLocation,
    categories: &'a [ProductCategory],
    storefront_key: &'a ContractKey,
    updated_at: &'a DateTime<Utc>,
}

/// The full directory state: a map of supplier entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DirectoryState {
    pub entries: BTreeMap<SupplierId, DirectoryEntry>,
}

impl DirectoryState {
    /// Merge another directory state into this one.
    /// Uses set-union with Last-Writer-Wins per supplier (by `updated_at`).
    pub fn merge(&mut self, other: DirectoryState) {
        for (id, entry) in other.entries {
            match self.entries.get(&id) {
                Some(existing) if existing.updated_at >= entry.updated_at => {
                    // Keep existing (it's newer or same age)
                }
                _ => {
                    self.entries.insert(id, entry);
                }
            }
        }
    }

    /// Validate all entries have correct signatures.
    pub fn validate_all_signatures(&self) -> bool {
        self.entries.values().all(|e| e.verify_signature())
    }
}

/// Summary of directory state: supplier ID -> last updated timestamp.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DirectorySummary {
    pub timestamps: BTreeMap<SupplierId, DateTime<Utc>>,
}

impl DirectoryState {
    pub fn summarize(&self) -> DirectorySummary {
        DirectorySummary {
            timestamps: self
                .entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.updated_at))
                .collect(),
        }
    }

    /// Compute a delta: entries in self that are newer than what the summary reports.
    pub fn delta(&self, summary: &DirectorySummary) -> DirectoryState {
        let entries = self
            .entries
            .iter()
            .filter(|(id, entry)| {
                summary
                    .timestamps
                    .get(*id)
                    .is_none_or(|ts| entry.updated_at > *ts)
            })
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect();
        DirectoryState { entries }
    }
}
