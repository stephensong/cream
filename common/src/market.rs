use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use ed25519_dalek::Signature;
#[cfg(not(feature = "dev"))]
use ed25519_dalek::Verifier;
use serde::{Deserialize, Serialize};

use crate::identity::UserId;
use crate::location::GeoLocation;

/// A single scheduled market event (one day).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketEvent {
    pub date: NaiveDate,
    pub start_time: String, // "07:00" (24h)
    pub end_time: String,   // "13:00" (24h)
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Invitation status for a supplier at a market.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SupplierStatus {
    Invited,
    Accepted,
}

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
    /// Scheduled event dates for this market.
    #[serde(default)]
    pub events: Vec<MarketEvent>,
    #[serde(default)]
    pub timezone: Option<String>,
    /// Supplier names → invite/accept status.
    #[serde(default)]
    pub suppliers: BTreeMap<String, SupplierStatus>,
    pub updated_at: DateTime<Utc>,
    pub signature: Signature,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
            events: &self.events,
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

    /// Return the next upcoming event on or after `today`, if any.
    pub fn next_event(&self, today: NaiveDate) -> Option<&MarketEvent> {
        self.events
            .iter()
            .filter(|e| e.date >= today)
            .min_by_key(|e| e.date)
    }

    /// Return names of suppliers who have accepted the invitation.
    pub fn accepted_suppliers(&self) -> Vec<&String> {
        self.suppliers
            .iter()
            .filter(|(_, status)| **status == SupplierStatus::Accepted)
            .map(|(name, _)| name)
            .collect()
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
    events: &'a Vec<MarketEvent>,
    timezone: Option<&'a str>,
    suppliers: &'a BTreeMap<String, SupplierStatus>,
    updated_at: &'a DateTime<Utc>,
}

/// The full market directory state: one market per organizer (v1).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketDirectoryState {
    pub entries: BTreeMap<UserId, MarketEntry>,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl MarketDirectoryState {
    pub fn summarize(&self) -> MarketDirectorySummary {
        MarketDirectorySummary {
            timestamps: self
                .entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.updated_at))
                .collect(),
            extra: Default::default(),
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
        MarketDirectoryState {
            entries,
            extra: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            events: vec![
                MarketEvent {
                    date: NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
                    start_time: "07:00".to_string(),
                    end_time: "13:00".to_string(),
                    extra: Default::default(),
                },
            ],
            timezone: Some("Australia/Sydney".to_string()),
            suppliers: BTreeMap::from([
                ("Gary".to_string(), SupplierStatus::Accepted),
                ("Emma".to_string(), SupplierStatus::Invited),
            ]),
            updated_at: ts,
            signature: Signature::from_bytes(&[0u8; 64]),
            extra: Default::default(),
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

    #[test]
    fn test_next_event() {
        let market = dummy_market("Test", Utc::now());
        let today = NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let next = market.next_event(today);
        assert!(next.is_some());
        assert_eq!(next.unwrap().date, NaiveDate::from_ymd_opt(2026, 3, 15).unwrap());

        // After all events
        let future = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
        assert!(market.next_event(future).is_none());
    }

    #[test]
    fn test_accepted_suppliers() {
        let market = dummy_market("Test", Utc::now());
        let accepted = market.accepted_suppliers();
        assert_eq!(accepted.len(), 1);
        assert_eq!(*accepted[0], "Gary");
    }
}
