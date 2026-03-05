use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
#[cfg(not(feature = "dev"))]
use ed25519_dalek::Verifier;
use serde::{Deserialize, Serialize};

use crate::identity::UserId;
use crate::location::GeoLocation;
use crate::storefront::WeeklySchedule;

/// A single market listing in the market directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEntry {
    pub organizer: UserId,
    pub name: String,
    pub description: String,
    pub venue_address: String,
    pub location: GeoLocation,
    #[serde(default)]
    pub postcode: Option<String>,
    #[serde(default)]
    pub locality: Option<String>,
    pub schedule: WeeklySchedule,
    #[serde(default)]
    pub timezone: Option<String>,
    /// Supplier names (matching DirectoryEntry.name) participating in this market.
    pub suppliers: BTreeSet<String>,
    pub updated_at: DateTime<Utc>,
    pub signature: Signature,
}

impl MarketEntry {
    /// Serialize the signable fields (everything except signature).
    pub fn signable_bytes(&self) -> Vec<u8> {
        let signable = SignableMarketEntry {
            organizer: &self.organizer,
            name: &self.name,
            description: &self.description,
            venue_address: &self.venue_address,
            location: &self.location,
            postcode: self.postcode.as_deref(),
            locality: self.locality.as_deref(),
            schedule: &self.schedule,
            timezone: self.timezone.as_deref(),
            suppliers: &self.suppliers,
            updated_at: &self.updated_at,
        };
        serde_json::to_vec(&signable).expect("serialization should not fail")
    }

    /// Verify that the entry was signed by the organizer's key.
    pub fn verify_signature(&self) -> bool {
        #[cfg(feature = "dev")]
        {
            #[allow(clippy::needless_return)]
            return true;
        }
        #[cfg(not(feature = "dev"))]
        {
            let msg = self.signable_bytes();
            self.organizer.0.verify(&msg, &self.signature).is_ok()
        }
    }
}

#[derive(Serialize)]
struct SignableMarketEntry<'a> {
    organizer: &'a UserId,
    name: &'a str,
    description: &'a str,
    venue_address: &'a str,
    location: &'a GeoLocation,
    postcode: Option<&'a str>,
    locality: Option<&'a str>,
    schedule: &'a WeeklySchedule,
    timezone: Option<&'a str>,
    suppliers: &'a BTreeSet<String>,
    updated_at: &'a DateTime<Utc>,
}

/// The full market directory state: one market per organizer (v1).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketDirectoryState {
    pub entries: BTreeMap<UserId, MarketEntry>,
}

impl MarketDirectoryState {
    /// Merge another market directory state into this one.
    /// Uses Last-Writer-Wins per organizer (by `updated_at`).
    pub fn merge(&mut self, other: MarketDirectoryState) {
        for (id, entry) in other.entries {
            match self.entries.get(&id) {
                Some(existing) if existing.updated_at >= entry.updated_at => {}
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

/// Summary of market directory state: organizer ID -> last updated timestamp.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketDirectorySummary {
    pub timestamps: BTreeMap<UserId, DateTime<Utc>>,
}

impl MarketDirectoryState {
    pub fn summarize(&self) -> MarketDirectorySummary {
        MarketDirectorySummary {
            timestamps: self
                .entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.updated_at))
                .collect(),
        }
    }

    /// Compute a delta: entries in self that are newer than what the summary reports.
    pub fn delta(&self, summary: &MarketDirectorySummary) -> MarketDirectoryState {
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
        MarketDirectoryState { entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storefront::WeeklySchedule;

    fn dummy_market(name: &str, ts: DateTime<Utc>) -> MarketEntry {
        use ed25519_dalek::Signature;
        MarketEntry {
            organizer: crate::identity::UserId(
                ed25519_dalek::VerifyingKey::from_bytes(&[1u8; 32]).unwrap(),
            ),
            name: name.to_string(),
            description: "A test market".to_string(),
            venue_address: "123 Market St".to_string(),
            location: GeoLocation::new(-33.8688, 151.2093),
            postcode: Some("2000".to_string()),
            locality: Some("Sydney".to_string()),
            schedule: WeeklySchedule::default(),
            timezone: Some("Australia/Sydney".to_string()),
            suppliers: BTreeSet::from(["Gary".to_string(), "Emma".to_string()]),
            updated_at: ts,
            signature: Signature::from_bytes(&[0u8; 64]),
        }
    }

    #[test]
    fn test_merge_lww() {
        let user_id = crate::identity::UserId(
            ed25519_dalek::VerifyingKey::from_bytes(&[1u8; 32]).unwrap(),
        );
        let t1 = Utc::now() - chrono::Duration::seconds(10);
        let t2 = Utc::now();

        let mut state = MarketDirectoryState::default();
        state.entries.insert(user_id.clone(), dummy_market("Old", t1));

        let mut other = MarketDirectoryState::default();
        other.entries.insert(user_id.clone(), dummy_market("New", t2));

        state.merge(other);
        assert_eq!(state.entries[&user_id].name, "New");
    }

    #[test]
    fn test_merge_keeps_newer() {
        let user_id = crate::identity::UserId(
            ed25519_dalek::VerifyingKey::from_bytes(&[1u8; 32]).unwrap(),
        );
        let t1 = Utc::now();
        let t2 = Utc::now() - chrono::Duration::seconds(10);

        let mut state = MarketDirectoryState::default();
        state.entries.insert(user_id.clone(), dummy_market("Existing", t1));

        let mut other = MarketDirectoryState::default();
        other.entries.insert(user_id.clone(), dummy_market("Older", t2));

        state.merge(other);
        assert_eq!(state.entries[&user_id].name, "Existing");
    }

    #[test]
    fn test_summarize_and_delta() {
        let user_id = crate::identity::UserId(
            ed25519_dalek::VerifyingKey::from_bytes(&[1u8; 32]).unwrap(),
        );
        let t1 = Utc::now() - chrono::Duration::seconds(10);
        let t2 = Utc::now();

        let mut state = MarketDirectoryState::default();
        state.entries.insert(user_id.clone(), dummy_market("Market", t2));

        // Summary with older timestamp → delta should include the entry
        let mut summary = MarketDirectorySummary::default();
        summary.timestamps.insert(user_id.clone(), t1);

        let delta = state.delta(&summary);
        assert_eq!(delta.entries.len(), 1);

        // Summary with same timestamp → delta should be empty
        summary.timestamps.insert(user_id.clone(), t2);
        let delta = state.delta(&summary);
        assert_eq!(delta.entries.len(), 0);
    }
}
