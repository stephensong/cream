#![cfg(feature = "multi-node-tests")]

//! Multi-node smoke tests.
//!
//! Requires a local Freenet network with 1 gateway + 2 nodes
//! (ports 3001, 3002, 3003). Start with `cargo make reset-network`.

use std::collections::BTreeMap;
use std::time::Duration;

use cream_common::directory::DirectoryState;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest, ContractResponse, HostResponse};
use freenet_stdlib::prelude::*;

use cream_node_integration::*;

const PROPAGATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Retry GET on a contract until it succeeds or the timeout expires.
/// Returns the state bytes on success, None on timeout.
async fn wait_for_get(
    api: &mut freenet_stdlib::client_api::WebApi,
    key: ContractInstanceId,
    timeout: Duration,
) -> Option<Vec<u8>> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }

        api.send(ClientRequest::ContractOp(ContractRequest::Get {
            key,
            return_contract_code: false,
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

        match tokio::time::timeout(Duration::from_secs(5), api.recv()).await {
            Ok(Ok(HostResponse::ContractResponse(ContractResponse::GetResponse {
                state, ..
            }))) => {
                return Some(state.as_ref().to_vec());
            }
            Ok(Ok(other)) => {
                tracing::debug!("wait_for_get: non-GET response: {:?}", other);
            }
            Ok(Err(e)) => {
                tracing::debug!("wait_for_get: error: {:?}", e);
            }
            Err(_) => {
                tracing::debug!("wait_for_get: recv timeout, will retry");
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_node_put_get_and_subscribe() {
    tracing_subscriber::fmt::try_init().ok();

    let url_gw = node_url(3001);
    let url_b = node_url(3003);

    // ═══════════════════════════════════════════════════════════════════
    // Step 1: Cross-node PUT/GET
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 1: cross-node PUT/GET ──");

    let mut client_gw = connect_to_node_at(&url_gw).await;
    let mut client_b = connect_to_node_at(&url_b).await;

    let (dir_contract, dir_key) = make_directory_contract();
    let empty_dir = DirectoryState::default();
    let state_bytes = serde_json::to_vec(&empty_dir).unwrap();

    // PUT the directory contract via the gateway (port 3001)
    client_gw
        .send(ClientRequest::ContractOp(ContractRequest::Put {
            contract: dir_contract,
            state: WrappedState::new(state_bytes),
            related_contracts: RelatedContracts::default(),
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

    let put_resp = recv_matching(&mut client_gw, is_put_response, PROPAGATION_TIMEOUT).await;
    assert!(
        put_resp.is_some(),
        "Expected PutResponse on gateway (port 3001) for directory"
    );
    println!("   PUT succeeded on gateway (port 3001)");

    // client_b GETs the contract from node 3003 (retry until propagated)
    let state = wait_for_get(&mut client_b, *dir_key.id(), PROPAGATION_TIMEOUT).await;
    assert!(
        state.is_some(),
        "Contract should propagate from node 3002 to node 3003 within {PROPAGATION_TIMEOUT:?}"
    );

    let dir: DirectoryState = serde_json::from_slice(&state.unwrap()).unwrap();
    assert!(
        dir.entries.is_empty(),
        "Initial directory state should be empty"
    );
    println!("   GET succeeded on node 3003 — contract propagated");
    println!("   PASSED");

    // ═══════════════════════════════════════════════════════════════════
    // Step 2: Cross-node subscription notification
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 2: cross-node subscription notification ──");

    // client_b subscribes to the directory on node 3003
    client_b
        .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
            key: *dir_key.id(),
            summary: None,
        }))
        .await
        .unwrap();

    let sub_resp = recv_matching(&mut client_b, is_subscribe_success, PROPAGATION_TIMEOUT).await;
    assert!(
        sub_resp.is_some(),
        "Expected SubscribeResponse on node 3003"
    );
    println!("   Subscribed on node 3003");

    // Update the directory via the gateway with a new supplier
    let (supplier_id, vk) = make_dummy_supplier("Multi-Node Farm");
    let (_, sf_key) = make_storefront_contract(&vk);
    let entry = make_directory_entry(&supplier_id, "Multi-Node Farm", sf_key);

    let mut entries = BTreeMap::new();
    entries.insert(supplier_id, entry);
    let delta = DirectoryState { entries };
    let delta_bytes = serde_json::to_vec(&delta).unwrap();

    client_gw
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: dir_key,
            data: UpdateData::Delta(StateDelta::from(delta_bytes)),
        }))
        .await
        .unwrap();
    println!("   Update sent from gateway (port 3001)");

    // client_b should receive an UpdateNotification on node 3003
    let notification =
        recv_matching(&mut client_b, is_update_notification, PROPAGATION_TIMEOUT).await;
    assert!(
        notification.is_some(),
        "Node 3003 subscriber should receive UpdateNotification from node 3002 within {PROPAGATION_TIMEOUT:?}"
    );

    let bytes = extract_notification_bytes(&notification.unwrap()).unwrap();
    let updated: DirectoryState = serde_json::from_slice(&bytes).unwrap();
    assert!(
        updated
            .entries
            .values()
            .any(|e| e.name == "Multi-Node Farm"),
        "Notification should contain 'Multi-Node Farm' entry"
    );
    println!("   Notification received on node 3003 with correct data");
    println!("   PASSED");

    println!("\n══ All multi-node smoke tests passed ══");
}
