use cream_common::directory::DirectoryState;
use cream_common::inbox::InboxState;
use cream_common::market::MarketDirectoryState;
use cream_common::storefront::{StorefrontParameters, StorefrontState};
use cream_common::user_contract::{UserContractParameters, UserContractState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractType {
    Directory,
    Storefront,
    UserContract,
    Inbox,
    MarketDirectory,
}

/// Owner key extracted from contract parameters, needed for validation.
#[allow(dead_code)]
pub enum OwnerKey {
    None,
    Key(ed25519_dalek::VerifyingKey),
}

/// Classify a contract by attempting to deserialize its state.
///
/// StorefrontParameters, UserContractParameters, and InboxParameters all have
/// identical structure (`{ owner: VerifyingKey }`), so we must disambiguate by
/// checking the state shape:
///
/// 1. Has an `owner` param → try state as StorefrontState / UserContractState / InboxState
/// 2. Empty/absent params → try Directory vs MarketDirectory by state shape
pub fn classify(params_bytes: &[u8], state_bytes: &[u8]) -> (ContractType, OwnerKey) {
    // If params deserializes as { owner: VerifyingKey }, disambiguate by state shape
    if let Ok(sp) = serde_json::from_slice::<StorefrontParameters>(params_bytes) {
        let owner = OwnerKey::Key(sp.owner);

        // StorefrontState has `info` field
        if serde_json::from_slice::<StorefrontState>(state_bytes).is_ok() {
            return (ContractType::Storefront, owner);
        }
        // UserContractState has `ledger` field
        if serde_json::from_slice::<UserContractState>(state_bytes).is_ok() {
            return (ContractType::UserContract, owner);
        }
        // InboxState has `messages` field
        if serde_json::from_slice::<InboxState>(state_bytes).is_ok() {
            return (ContractType::Inbox, owner);
        }
        // Fallback: could be any of the three — check JSON for distinguishing fields
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(state_bytes) {
            if v.get("info").is_some() {
                return (ContractType::Storefront, owner);
            }
            if v.get("ledger").is_some() {
                return (ContractType::UserContract, owner);
            }
            if v.get("messages").is_some() {
                return (ContractType::Inbox, owner);
            }
        }
        // Default for owner-param contracts
        return (ContractType::Storefront, owner);
    }

    // Empty params — distinguish Directory vs MarketDirectory by state shape
    if serde_json::from_slice::<MarketDirectoryState>(state_bytes)
        .map(|s| !s.entries.is_empty())
        .unwrap_or(false)
    {
        return (ContractType::MarketDirectory, OwnerKey::None);
    }

    // For empty states, try to detect MarketDirectory by attempting Directory parse.
    // Both are maps but with different value types. Default to Directory.
    // MarketDirectory entries have "venue_address" field; Directory entries have "storefront_key".
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(state_bytes) {
        if let Some(obj) = v.as_object() {
            if let Some(entries) = obj.get("entries") {
                if let Some(map) = entries.as_object() {
                    for (_, entry) in map.iter() {
                        if entry.get("venue_address").is_some() {
                            return (ContractType::MarketDirectory, OwnerKey::None);
                        }
                        if entry.get("storefront_key").is_some() {
                            return (ContractType::Directory, OwnerKey::None);
                        }
                    }
                }
            }
        }
    }

    (ContractType::Directory, OwnerKey::None)
}

/// Validate and merge an update into existing state. Returns new serialized state bytes.
pub fn apply_update(
    contract_type: ContractType,
    params_bytes: &[u8],
    current_state_bytes: &[u8],
    update_bytes: &[u8],
) -> Result<Vec<u8>, ContractError> {
    if update_bytes.is_empty() {
        return Ok(current_state_bytes.to_vec());
    }

    match contract_type {
        ContractType::Directory => {
            // Directory and MarketDirectory have identical empty params and empty
            // initial state, so a contract initially classified as Directory may
            // actually be a MarketDirectory. Try Directory first; if the update
            // fails to parse, fall through to MarketDirectory.
            if let Ok(update) = serde_json::from_slice::<DirectoryState>(update_bytes) {
                let mut state: DirectoryState = serde_json::from_slice(current_state_bytes)
                    .map_err(|e| ContractError::InvalidState(e.to_string()))?;
                if !update.validate_all_signatures() {
                    return Err(ContractError::ValidationFailed(
                        "directory signature validation failed".into(),
                    ));
                }
                state.merge(update);
                return serde_json::to_vec(&state)
                    .map_err(|e| ContractError::Serialization(e.to_string()));
            }
            // Fall through to MarketDirectory
            let mut state: MarketDirectoryState = serde_json::from_slice(current_state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            let update: MarketDirectoryState = serde_json::from_slice(update_bytes)
                .map_err(|e| ContractError::InvalidUpdate(e.to_string()))?;
            if !update.validate_all_signatures() {
                return Err(ContractError::ValidationFailed(
                    "market directory signature validation failed".into(),
                ));
            }
            state.merge(update);
            serde_json::to_vec(&state).map_err(|e| ContractError::Serialization(e.to_string()))
        }

        ContractType::Storefront => {
            let owner = extract_storefront_owner(params_bytes)?;
            let mut state: StorefrontState = serde_json::from_slice(current_state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            let update: StorefrontState = serde_json::from_slice(update_bytes)
                .map_err(|e| ContractError::InvalidUpdate(e.to_string()))?;
            if !update.validate(&owner) {
                return Err(ContractError::ValidationFailed(
                    "storefront validation failed".into(),
                ));
            }
            state.merge(update);
            serde_json::to_vec(&state).map_err(|e| ContractError::Serialization(e.to_string()))
        }

        ContractType::UserContract => {
            let owner = extract_user_contract_owner(params_bytes)?;
            let mut state: UserContractState = serde_json::from_slice(current_state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            let update: UserContractState = serde_json::from_slice(update_bytes)
                .map_err(|e| ContractError::InvalidUpdate(e.to_string()))?;
            if !state.validate_update(&update, &owner) {
                return Err(ContractError::ValidationFailed(
                    "user contract validation failed".into(),
                ));
            }
            state.merge(update);
            serde_json::to_vec(&state).map_err(|e| ContractError::Serialization(e.to_string()))
        }

        ContractType::Inbox => {
            let mut state: InboxState = serde_json::from_slice(current_state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            let update: InboxState = serde_json::from_slice(update_bytes)
                .map_err(|e| ContractError::InvalidUpdate(e.to_string()))?;
            if !state.validate_update(&update) {
                return Err(ContractError::ValidationFailed(
                    "inbox validation failed".into(),
                ));
            }
            state.merge(update);
            serde_json::to_vec(&state).map_err(|e| ContractError::Serialization(e.to_string()))
        }

        ContractType::MarketDirectory => {
            let mut state: MarketDirectoryState = serde_json::from_slice(current_state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            let update: MarketDirectoryState = serde_json::from_slice(update_bytes)
                .map_err(|e| ContractError::InvalidUpdate(e.to_string()))?;
            if !update.validate_all_signatures() {
                return Err(ContractError::ValidationFailed(
                    "market directory signature validation failed".into(),
                ));
            }
            state.merge(update);
            serde_json::to_vec(&state).map_err(|e| ContractError::Serialization(e.to_string()))
        }
    }
}

/// Validate initial state for a PUT operation.
pub fn validate_state(
    contract_type: ContractType,
    params_bytes: &[u8],
    state_bytes: &[u8],
) -> Result<bool, ContractError> {
    match contract_type {
        ContractType::Directory => {
            let state: DirectoryState = serde_json::from_slice(state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            Ok(state.validate_all_signatures())
        }
        ContractType::Storefront => {
            let owner = extract_storefront_owner(params_bytes)?;
            let state: StorefrontState = serde_json::from_slice(state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            Ok(state.validate(&owner))
        }
        ContractType::UserContract => {
            // Initial state just needs to deserialize correctly
            let _state: UserContractState = serde_json::from_slice(state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            Ok(true)
        }
        ContractType::Inbox => {
            let _state: InboxState = serde_json::from_slice(state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            Ok(true)
        }
        ContractType::MarketDirectory => {
            let state: MarketDirectoryState = serde_json::from_slice(state_bytes)
                .map_err(|e| ContractError::InvalidState(e.to_string()))?;
            Ok(state.validate_all_signatures())
        }
    }
}

fn extract_storefront_owner(params_bytes: &[u8]) -> Result<ed25519_dalek::VerifyingKey, ContractError> {
    let params: StorefrontParameters = serde_json::from_slice(params_bytes)
        .map_err(|e| ContractError::InvalidState(format!("bad storefront params: {e}")))?;
    Ok(params.owner)
}

fn extract_user_contract_owner(params_bytes: &[u8]) -> Result<ed25519_dalek::VerifyingKey, ContractError> {
    let params: UserContractParameters = serde_json::from_slice(params_bytes)
        .map_err(|e| ContractError::InvalidState(format!("bad user contract params: {e}")))?;
    Ok(params.owner)
}

#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("invalid update: {0}")]
    InvalidUpdate(String),
    #[error("validation failed: {0}")]
    ValidationFailed(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}
