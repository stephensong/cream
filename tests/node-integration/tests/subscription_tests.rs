#![cfg(feature = "node-tests")]

use std::collections::BTreeMap;
use std::time::Duration;

use cream_common::directory::DirectoryState;
use cream_common::storefront::StorefrontState;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest};
use freenet_stdlib::prelude::*;

use cream_node_integration::*;

const TIMEOUT: Duration = Duration::from_secs(5);

/// Test 1: Subscribe to directory, update it, receive notification.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn directory_subscribe_notifies_on_update() {
    tracing_subscriber::fmt::try_init().ok();

    let mut client_a = connect_to_node().await;
    let mut client_b = connect_to_node().await;

    // Client A: PUT directory contract with empty state
    let (dir_contract, dir_key) = make_directory_contract();
    let empty_dir = DirectoryState::default();
    let state_bytes = serde_json::to_vec(&empty_dir).unwrap();

    client_a
        .send(ClientRequest::ContractOp(ContractRequest::Put {
            contract: dir_contract,
            state: WrappedState::new(state_bytes),
            related_contracts: RelatedContracts::default(),
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

    // Wait for PutResponse
    let put_resp = recv_matching(&mut client_a, is_put_response, TIMEOUT).await;
    assert!(put_resp.is_some(), "Expected PutResponse for directory");

    // Client B: explicit Subscribe to directory
    client_b
        .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
            key: *dir_key.id(),
            summary: None,
        }))
        .await
        .unwrap();

    // Client B: wait for SubscribeResponse
    let sub_resp = recv_matching(&mut client_b, is_subscribe_success, TIMEOUT).await;
    assert!(
        sub_resp.is_some(),
        "Expected SubscribeResponse for directory"
    );

    // Client A: UPDATE directory with a new supplier entry
    let (supplier_id, vk) = make_dummy_supplier();
    let (_, sf_key) = make_storefront_contract(&vk);
    let entry = make_directory_entry(&supplier_id, "Test Farm", sf_key);

    let mut entries = BTreeMap::new();
    entries.insert(supplier_id, entry);
    let delta = DirectoryState { entries };
    let delta_bytes = serde_json::to_vec(&delta).unwrap();

    client_a
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: dir_key,
            data: UpdateData::Delta(StateDelta::from(delta_bytes)),
        }))
        .await
        .unwrap();

    // Client B: wait for UpdateNotification
    let notification = recv_matching(&mut client_b, is_update_notification, TIMEOUT).await;
    assert!(
        notification.is_some(),
        "Client B should receive UpdateNotification for directory"
    );

    // Verify notification contains the new entry
    let bytes = extract_notification_bytes(&notification.unwrap()).unwrap();
    let updated: DirectoryState = serde_json::from_slice(&bytes).unwrap();
    assert!(
        updated.entries.values().any(|e| e.name == "Test Farm"),
        "Notification should contain 'Test Farm' entry"
    );
}

/// Test 2: Subscribe to storefront, add product, receive notification.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn storefront_subscribe_notifies_on_product_add() {
    tracing_subscriber::fmt::try_init().ok();

    let mut client_a = connect_to_node().await;
    let mut client_b = connect_to_node().await;

    let (supplier_id, vk) = make_dummy_supplier();
    let (sf_contract, sf_key) = make_storefront_contract(&vk);

    // Client A: PUT storefront with initial state (empty products)
    let initial_sf = StorefrontState {
        info: cream_common::storefront::StorefrontInfo {
            owner: supplier_id,
            name: "Test Farm".to_string(),
            description: "A test farm".to_string(),
            location: cream_common::location::GeoLocation::new(-33.87, 151.21),
        },
        products: BTreeMap::new(),
        orders: BTreeMap::new(),
    };
    let state_bytes = serde_json::to_vec(&initial_sf).unwrap();

    client_a
        .send(ClientRequest::ContractOp(ContractRequest::Put {
            contract: sf_contract,
            state: WrappedState::new(state_bytes),
            related_contracts: RelatedContracts::default(),
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

    // Wait for PutResponse
    let put_resp = recv_matching(&mut client_a, is_put_response, TIMEOUT).await;
    assert!(put_resp.is_some(), "Expected PutResponse for storefront");

    // Client B: explicit Subscribe to storefront
    client_b
        .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
            key: *sf_key.id(),
            summary: None,
        }))
        .await
        .unwrap();

    // Client B: wait for SubscribeResponse
    let sub_resp = recv_matching(&mut client_b, is_subscribe_success, TIMEOUT).await;
    assert!(
        sub_resp.is_some(),
        "Expected SubscribeResponse for storefront"
    );

    // Client A: UPDATE storefront with a new product
    let product = make_dummy_product("Raw Milk");
    let mut updated_sf = initial_sf.clone();
    updated_sf
        .products
        .insert(product.product.id.clone(), product);
    let sf_bytes = serde_json::to_vec(&updated_sf).unwrap();

    client_a
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: sf_key,
            data: UpdateData::State(State::from(sf_bytes)),
        }))
        .await
        .unwrap();

    // Client B: wait for UpdateNotification
    let notification = recv_matching(&mut client_b, is_update_notification, TIMEOUT).await;
    assert!(
        notification.is_some(),
        "Client B should receive UpdateNotification for storefront product add"
    );

    // Verify the notification contains the new product
    let bytes = extract_notification_bytes(&notification.unwrap()).unwrap();
    let sf: StorefrontState = serde_json::from_slice(&bytes).unwrap();
    assert!(
        sf.products.values().any(|sp| sp.product.name == "Raw Milk"),
        "Notification should contain 'Raw Milk' product"
    );
}

/// Test 3: Diagnostic — does GET with subscribe:true actually produce notifications?
/// This documents the actual behavior to guide our UI implementation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_subscribe_flag_vs_explicit_subscribe() {
    tracing_subscriber::fmt::try_init().ok();

    let mut client_a = connect_to_node().await;
    let mut client_b_get = connect_to_node().await;
    let mut client_b_sub = connect_to_node().await;

    let (supplier_id, vk) = make_dummy_supplier();
    let (sf_contract, sf_key) = make_storefront_contract(&vk);

    // Client A: PUT storefront
    let initial_sf = StorefrontState {
        info: cream_common::storefront::StorefrontInfo {
            owner: supplier_id,
            name: "Diagnostic Farm".to_string(),
            description: "Testing subscription methods".to_string(),
            location: cream_common::location::GeoLocation::new(-33.87, 151.21),
        },
        products: BTreeMap::new(),
        orders: BTreeMap::new(),
    };
    let state_bytes = serde_json::to_vec(&initial_sf).unwrap();

    client_a
        .send(ClientRequest::ContractOp(ContractRequest::Put {
            contract: sf_contract,
            state: WrappedState::new(state_bytes),
            related_contracts: RelatedContracts::default(),
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

    recv_matching(&mut client_a, is_put_response, TIMEOUT)
        .await
        .expect("PutResponse");

    // Client B (GET method): subscribe via GET flag
    client_b_get
        .send(ClientRequest::ContractOp(ContractRequest::Get {
            key: *sf_key.id(),
            return_contract_code: false,
            subscribe: true,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

    // Wait for GetResponse
    recv_matching(&mut client_b_get, is_get_response, TIMEOUT)
        .await
        .expect("GetResponse for GET+subscribe");

    // Client B (Subscribe method): explicit Subscribe
    client_b_sub
        .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
            key: *sf_key.id(),
            summary: None,
        }))
        .await
        .unwrap();

    // Wait for SubscribeResponse
    recv_matching(&mut client_b_sub, is_subscribe_success, TIMEOUT)
        .await
        .expect("SubscribeResponse for explicit Subscribe");

    // Client A: UPDATE with new product
    let product = make_dummy_product("Test Cheese");
    let mut updated_sf = initial_sf.clone();
    updated_sf
        .products
        .insert(product.product.id.clone(), product);
    let sf_bytes = serde_json::to_vec(&updated_sf).unwrap();

    client_a
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: sf_key,
            data: UpdateData::State(State::from(sf_bytes)),
        }))
        .await
        .unwrap();

    // Check which clients receive the notification
    let short_timeout = Duration::from_secs(3);

    let got_via_get_flag =
        recv_matching(&mut client_b_get, is_update_notification, short_timeout).await;
    let got_via_explicit_sub =
        recv_matching(&mut client_b_sub, is_update_notification, short_timeout).await;

    println!("=== Subscription Method Comparison ===");
    println!(
        "GET with subscribe:true → notification received: {}",
        got_via_get_flag.is_some()
    );
    println!(
        "Explicit Subscribe      → notification received: {}",
        got_via_explicit_sub.is_some()
    );

    // At least one method should work
    assert!(
        got_via_get_flag.is_some() || got_via_explicit_sub.is_some(),
        "Neither subscription method produced an UpdateNotification — \
         this suggests a Freenet node issue"
    );
}

/// Test 4: Subscriber sees product count go from 0 → 1 → 2 as supplier adds products.
///
/// This reproduces the bug where Tab B's directory view showed stale product counts
/// after Tab A added a product. The directory view derives its count from
/// `storefront.products.len()`, so the storefront notification must contain the
/// full merged state for the count to update correctly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn product_count_increments_for_subscriber() {
    tracing_subscriber::fmt::try_init().ok();

    let mut supplier = connect_to_node().await;
    let mut customer = connect_to_node().await;

    let (supplier_id, vk) = make_dummy_supplier();
    let (sf_contract, sf_key) = make_storefront_contract(&vk);

    // Supplier: PUT storefront with 0 products
    let initial_sf = StorefrontState {
        info: cream_common::storefront::StorefrontInfo {
            owner: supplier_id,
            name: "Count Farm".to_string(),
            description: "Testing product count updates".to_string(),
            location: cream_common::location::GeoLocation::new(-33.87, 151.21),
        },
        products: BTreeMap::new(),
        orders: BTreeMap::new(),
    };
    let state_bytes = serde_json::to_vec(&initial_sf).unwrap();

    supplier
        .send(ClientRequest::ContractOp(ContractRequest::Put {
            contract: sf_contract,
            state: WrappedState::new(state_bytes),
            related_contracts: RelatedContracts::default(),
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

    recv_matching(&mut supplier, is_put_response, TIMEOUT)
        .await
        .expect("PutResponse for storefront");

    // Customer: GET storefront to see initial state (0 products)
    customer
        .send(ClientRequest::ContractOp(ContractRequest::Get {
            key: *sf_key.id(),
            return_contract_code: false,
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

    let get_resp = recv_matching(&mut customer, is_get_response, TIMEOUT)
        .await
        .expect("GetResponse for storefront");

    // Parse the initial state and verify 0 products
    let initial_state = extract_get_response_state(&get_resp).expect("state bytes from GET");
    let sf: StorefrontState =
        serde_json::from_slice(&initial_state).expect("deserialize storefront from GET");
    assert_eq!(
        sf.products.len(),
        0,
        "Initial storefront should have 0 products"
    );

    // Customer: explicit Subscribe to storefront
    customer
        .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
            key: *sf_key.id(),
            summary: None,
        }))
        .await
        .unwrap();

    recv_matching(&mut customer, is_subscribe_success, TIMEOUT)
        .await
        .expect("SubscribeResponse for storefront");

    // ── Supplier adds first product ──────────────────────────────────
    let product1 = make_dummy_product("Raw Milk");
    let mut sf_with_1 = initial_sf.clone();
    sf_with_1
        .products
        .insert(product1.product.id.clone(), product1);
    assert_eq!(sf_with_1.products.len(), 1);

    supplier
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: sf_key.clone(),
            data: UpdateData::State(State::from(serde_json::to_vec(&sf_with_1).unwrap())),
        }))
        .await
        .unwrap();

    // Customer: receive notification, verify product count = 1
    let notif1 = recv_matching(&mut customer, is_update_notification, TIMEOUT)
        .await
        .expect("Customer should receive notification after first product add");

    let bytes1 = extract_notification_bytes(&notif1).expect("notification bytes");
    let sf_notif1: StorefrontState =
        serde_json::from_slice(&bytes1).expect("deserialize storefront notification");

    println!(
        "After 1st add: notification contains {} products",
        sf_notif1.products.len()
    );
    assert_eq!(
        sf_notif1.products.len(),
        1,
        "After first product add, subscriber should see 1 product"
    );

    // ── Supplier adds second product ─────────────────────────────────
    // Small delay so the second product gets a distinct timestamp-based ID
    tokio::time::sleep(Duration::from_millis(10)).await;

    let product2 = make_dummy_product("Aged Cheddar");
    let mut sf_with_2 = sf_with_1.clone();
    sf_with_2
        .products
        .insert(product2.product.id.clone(), product2);
    assert_eq!(sf_with_2.products.len(), 2);

    supplier
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: sf_key,
            data: UpdateData::State(State::from(serde_json::to_vec(&sf_with_2).unwrap())),
        }))
        .await
        .unwrap();

    // Customer: receive notification, verify product count = 2
    let notif2 = recv_matching(&mut customer, is_update_notification, TIMEOUT)
        .await
        .expect("Customer should receive notification after second product add");

    let bytes2 = extract_notification_bytes(&notif2).expect("notification bytes");
    let sf_notif2: StorefrontState =
        serde_json::from_slice(&bytes2).expect("deserialize storefront notification");

    println!(
        "After 2nd add: notification contains {} products",
        sf_notif2.products.len()
    );
    assert_eq!(
        sf_notif2.products.len(),
        2,
        "After second product add, subscriber should see 2 products"
    );
}
