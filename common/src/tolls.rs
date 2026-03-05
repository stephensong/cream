use serde::{Deserialize, Serialize};

/// All guardian-configurable toll rates.
///
/// Guardians publish these rates at runtime via `GET /tolls`.
/// The UI fetches them at startup and uses them for all toll charges.
/// Defaults: everything 1 CURD.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TollRates {
    pub session_toll_curd: u64,
    pub session_interval_secs: u32,
    pub inbox_message_curd: u64,
    pub curd_per_sat: u64,
    /// Extension fields — preserves unknown fields across contract versions.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for TollRates {
    fn default() -> Self {
        Self {
            session_toll_curd: 1,
            session_interval_secs: 10,
            inbox_message_curd: 1,
            curd_per_sat: 10,
            extra: Default::default(),
        }
    }
}
