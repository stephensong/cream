use std::sync::Arc;
use std::time::Duration;

use freenet_stdlib::client_api::{ClientRequest, ContractRequest, ContractResponse, HostResponse, WebApi};
use freenet_stdlib::prelude::*;
use tokio::time::Instant;

use cream_common::directory::DirectoryEntry;
use cream_common::identity::{CustomerId, SupplierId};
use cream_common::location::GeoLocation;
use cream_common::order::{DepositTier, Order, OrderId, OrderStatus};
use cream_common::product::{Product, ProductCategory, ProductId};
use cream_common::storefront::{SignedProduct, StorefrontParameters};

pub mod harness;

/// Build a full WebSocket URL for a Freenet node on the given port.
pub fn node_url(port: u16) -> String {
    format!("ws://localhost:{port}/v1/contract/command?encodingProtocol=native")
}

/// Connect a native WebApi client to a Freenet node at an arbitrary URL.
pub async fn connect_to_node_at(url: &str) -> WebApi {
    let (ws_conn, _) = tokio_tungstenite::connect_async(url)
        .await
        .unwrap_or_else(|e| panic!("Failed to connect to Freenet node at {url}: {e}"));
    WebApi::start(ws_conn)
}

/// Build a ContractContainer from WASM bytes and parameters.
fn make_contract(wasm_bytes: &[u8], params: Parameters<'static>) -> ContractContainer {
    let code = ContractCode::from(wasm_bytes.to_vec());
    let wrapped = WrappedContract::new(Arc::new(code), params);
    ContractContainer::Wasm(ContractWasmAPIVersion::V1(wrapped))
}

/// Embedded contract WASM blobs (same ones the UI uses).
const DIRECTORY_WASM: &[u8] =
    include_bytes!("../../../target/wasm32-unknown-unknown/release/cream_directory_contract.wasm");
const STOREFRONT_WASM: &[u8] =
    include_bytes!("../../../target/wasm32-unknown-unknown/release/cream_storefront_contract.wasm");

/// Create a directory contract container + its key.
pub fn make_directory_contract() -> (ContractContainer, ContractKey) {
    let contract = make_contract(DIRECTORY_WASM, Parameters::from(vec![]));
    let key = contract.key();
    (contract, key)
}

/// Create a storefront contract container + its key for a given owner.
pub fn make_storefront_contract(
    owner: &ed25519_dalek::VerifyingKey,
) -> (ContractContainer, ContractKey) {
    let params = StorefrontParameters { owner: *owner };
    let params_bytes = serde_json::to_vec(&params).unwrap();
    let contract = make_contract(STOREFRONT_WASM, Parameters::from(params_bytes));
    let key = contract.key();
    (contract, key)
}

/// Create a deterministic supplier identity from a name.
///
/// Uses `derive_signing_key(name, password)` with `password = name.to_lowercase()`
/// so that the test harness produces the same keys as the UI when a user logs
/// in with their name as the password.
pub fn make_dummy_supplier(name: &str) -> (SupplierId, ed25519_dalek::VerifyingKey) {
    let password = name.to_lowercase();
    let signing_key = cream_common::identity::derive_supplier_signing_key(name, &password);
    let verifying_key = ed25519_dalek::VerifyingKey::from(&signing_key);
    (SupplierId(verifying_key), verifying_key)
}

/// Create a deterministic customer identity from a name.
pub fn make_dummy_customer(name: &str) -> (CustomerId, ed25519_dalek::VerifyingKey) {
    let password = name.to_lowercase();
    let signing_key = cream_common::identity::derive_customer_signing_key(name, &password);
    let verifying_key = ed25519_dalek::VerifyingKey::from(&signing_key);
    (CustomerId(verifying_key), verifying_key)
}

/// Create a dummy directory entry for a supplier.
pub fn make_directory_entry(
    supplier_id: &SupplierId,
    name: &str,
    storefront_key: ContractKey,
) -> DirectoryEntry {
    DirectoryEntry {
        supplier: supplier_id.clone(),
        name: name.to_string(),
        description: format!("{name}'s farm"),
        location: GeoLocation::new(-33.87, 151.21),
        postcode: Some("2000".to_string()),
        locality: Some("Sydney".to_string()),
        categories: vec![],
        storefront_key,
        updated_at: chrono::Utc::now(),
        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
    }
}

/// Create a dummy product.
pub fn make_dummy_product(name: &str) -> SignedProduct {
    let now = chrono::Utc::now();
    SignedProduct {
        product: Product {
            id: ProductId(format!("p-{}", now.timestamp_millis())),
            name: name.to_string(),
            description: format!("Fresh {name}"),
            category: ProductCategory::Milk,
            price_curd: 500,
            quantity_total: 10,
            expiry_date: None,
            updated_at: now,
            created_at: now,
        },
        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
    }
}

/// Create a dummy order with a configurable deposit tier and creation timestamp.
///
/// The `created_at` parameter lets tests create backdated orders whose reservation
/// has already expired relative to "now", enabling expiry tests without real waits.
pub fn make_dummy_order(
    product_id: &ProductId,
    customer_id: &CustomerId,
    tier: DepositTier,
    quantity: u32,
    price_per_unit: u64,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Order {
    let total_price = price_per_unit * quantity as u64;
    let deposit_amount = tier.calculate_deposit(total_price);
    let expires_at = match tier {
        DepositTier::Reserve2Days => created_at + chrono::Duration::days(2),
        DepositTier::Reserve1Week => created_at + chrono::Duration::weeks(1),
        DepositTier::FullPayment => created_at + chrono::Duration::days(365),
    };
    let order_id = OrderId(format!("o-{}", created_at.timestamp_millis()));
    Order {
        id: order_id,
        product_id: product_id.clone(),
        customer: customer_id.clone(),
        quantity,
        deposit_tier: tier,
        deposit_amount,
        total_price,
        status: OrderStatus::Reserved { expires_at },
        created_at,
        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
    }
}

/// Wait for a HostResponse matching a predicate, with timeout.
/// Non-matching responses are logged and discarded.
pub async fn recv_matching<F>(
    api: &mut WebApi,
    predicate: F,
    timeout: Duration,
) -> Option<HostResponse>
where
    F: Fn(&HostResponse) -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match tokio::time::timeout(remaining, api.recv()).await {
            Ok(Ok(resp)) if predicate(&resp) => return Some(resp),
            Ok(Ok(other)) => {
                tracing::debug!("Discarding non-matching response: {:?}", other);
                continue;
            }
            Ok(Err(e)) => {
                tracing::error!("Node error while waiting: {:?}", e);
                return None;
            }
            Err(_) => return None, // timeout
        }
    }
}

/// Check if a HostResponse is a ContractResponse::UpdateNotification.
pub fn is_update_notification(resp: &HostResponse) -> bool {
    matches!(
        resp,
        HostResponse::ContractResponse(ContractResponse::UpdateNotification { .. })
    )
}

/// Check if a HostResponse is a SubscribeResponse with subscribed=true.
pub fn is_subscribe_success(resp: &HostResponse) -> bool {
    matches!(
        resp,
        HostResponse::ContractResponse(ContractResponse::SubscribeResponse {
            subscribed: true,
            ..
        })
    )
}

/// Check if a HostResponse is a PutResponse.
pub fn is_put_response(resp: &HostResponse) -> bool {
    matches!(
        resp,
        HostResponse::ContractResponse(ContractResponse::PutResponse { .. })
    )
}

/// Check if a HostResponse is an UpdateResponse.
pub fn is_update_response(resp: &HostResponse) -> bool {
    matches!(
        resp,
        HostResponse::ContractResponse(ContractResponse::UpdateResponse { .. })
    )
}

/// Check if a HostResponse is a GetResponse.
pub fn is_get_response(resp: &HostResponse) -> bool {
    matches!(
        resp,
        HostResponse::ContractResponse(ContractResponse::GetResponse { .. })
    )
}

/// Extract the state bytes from a GetResponse.
pub fn extract_get_response_state(resp: &HostResponse) -> Option<Vec<u8>> {
    if let HostResponse::ContractResponse(ContractResponse::GetResponse { state, .. }) = resp {
        Some(state.as_ref().to_vec())
    } else {
        None
    }
}

/// Retry GET on a contract until it succeeds or the timeout expires.
/// Returns the state bytes on success, None on timeout.
pub async fn wait_for_get(
    api: &mut WebApi,
    key: ContractInstanceId,
    timeout: Duration,
) -> Option<Vec<u8>> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
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

/// Send PUT and drain messages until a PutResponse is received, or timeout expires.
/// If the recv times out without a PutResponse, re-sends the PUT and tries again.
pub async fn wait_for_put(
    api: &mut WebApi,
    contract: ContractContainer,
    state: WrappedState,
    timeout: Duration,
) -> Option<HostResponse> {
    let deadline = Instant::now() + timeout;
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            tracing::error!("wait_for_put: timed out after {attempt} attempts");
            return None;
        }

        api.send(ClientRequest::ContractOp(ContractRequest::Put {
            contract: contract.clone(),
            state: state.clone(),
            related_contracts: RelatedContracts::default(),
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

        // Drain messages for up to 30s looking for the PutResponse
        let drain_deadline = Instant::now() + Duration::from_secs(30).min(remaining);
        loop {
            let drain_remaining = drain_deadline.saturating_duration_since(Instant::now());
            if drain_remaining.is_zero() {
                tracing::debug!("wait_for_put: drain timeout on attempt {attempt}, will re-send");
                break;
            }
            match tokio::time::timeout(drain_remaining, api.recv()).await {
                Ok(Ok(resp)) if is_put_response(&resp) => {
                    if attempt > 1 {
                        tracing::info!("wait_for_put: succeeded on attempt {attempt}");
                    }
                    return Some(resp);
                }
                Ok(Ok(_other)) => {
                    // Non-PUT message (e.g. notification), keep draining
                    continue;
                }
                Ok(Err(e)) => {
                    tracing::debug!("wait_for_put: error on attempt {attempt}: {:?}", e);
                    break;
                }
                Err(_) => {
                    tracing::debug!("wait_for_put: drain timeout on attempt {attempt}, will re-send");
                    break;
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Extract UpdateNotification bytes from a HostResponse.
pub fn extract_notification_bytes(resp: &HostResponse) -> Option<Vec<u8>> {
    if let HostResponse::ContractResponse(ContractResponse::UpdateNotification { update, .. }) =
        resp
    {
        let bytes = match update {
            UpdateData::State(s) => s.as_ref().to_vec(),
            UpdateData::Delta(d) => d.as_ref().to_vec(),
            UpdateData::StateAndDelta { state, .. } => state.as_ref().to_vec(),
            _ => return None,
        };
        Some(bytes)
    } else {
        None
    }
}
