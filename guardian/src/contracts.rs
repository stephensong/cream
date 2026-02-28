//! Contract key helpers for guardian node monitoring.
//!
//! Derives deterministic contract keys for the directory and root user contracts
//! so the guardian can subscribe to them on its co-located Freenet node.

use std::sync::Arc;

use freenet_stdlib::prelude::*;
use frost_ed25519 as frost;

const DIRECTORY_WASM: &[u8] =
    include_bytes!("../../target/wasm32-unknown-unknown/release/cream_directory_contract.wasm");
const USER_CONTRACT_WASM: &[u8] =
    include_bytes!("../../target/wasm32-unknown-unknown/release/cream_user_contract.wasm");

fn make_contract(wasm_bytes: &[u8], params: Parameters<'static>) -> ContractContainer {
    let code = ContractCode::from(wasm_bytes.to_vec());
    let wrapped = WrappedContract::new(Arc::new(code), params);
    ContractContainer::Wasm(ContractWasmAPIVersion::V1(wrapped))
}

/// Deterministic directory contract key (empty parameters).
pub fn directory_contract_key() -> ContractKey {
    let contract = make_contract(DIRECTORY_WASM, Parameters::from(vec![]));
    contract.key()
}

/// Root user contract key derived from the FROST group verifying key.
pub fn root_user_contract_key(pubkey_package: &frost::keys::PublicKeyPackage) -> ContractKey {
    let vk = cream_common::frost::group_verifying_key(pubkey_package);
    let params = cream_common::user_contract::UserContractParameters { owner: vk };
    let params_bytes = serde_json::to_vec(&params).unwrap();
    let contract = make_contract(USER_CONTRACT_WASM, Parameters::from(params_bytes));
    contract.key()
}
