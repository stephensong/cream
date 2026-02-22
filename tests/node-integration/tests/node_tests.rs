#![cfg(feature = "node-tests")]

//! Cumulative node-integration tests.
//!
//! All steps run sequentially inside a single `#[tokio::test]`. Each step
//! assumes every previous step succeeded — if any step panics the entire
//! run stops immediately. The final node state serves as the fixture for
//! downstream E2E browser tests.

use std::collections::BTreeMap;
use std::time::Duration;

use cream_common::directory::DirectoryState;
use cream_common::product::ProductCategory;
use cream_common::storefront::StorefrontState;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest};
use freenet_stdlib::prelude::*;

use cream_node_integration::harness::TestHarness;
use cream_node_integration::*;

const TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cumulative_node_tests() {
    tracing_subscriber::fmt::try_init().ok();

    // ═══════════════════════════════════════════════════════════════════
    // Step 1: Directory subscribe → update → notification
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 1: directory_subscribe_notifies_on_update ──");
    {
        let mut client_a = connect_to_node().await;
        let mut client_b = connect_to_node().await;

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

        // Short timeout — directory may already exist from a previous run
        let _put_resp =
            recv_matching(&mut client_a, is_put_response, Duration::from_secs(2)).await;

        client_b
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *dir_key.id(),
                summary: None,
            }))
            .await
            .unwrap();

        let sub_resp = recv_matching(&mut client_b, is_subscribe_success, TIMEOUT).await;
        assert!(
            sub_resp.is_some(),
            "Expected SubscribeResponse for directory"
        );

        let (supplier_id, vk) = make_dummy_supplier("Test Farm");
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

        let notification = recv_matching(&mut client_b, is_update_notification, TIMEOUT).await;
        assert!(
            notification.is_some(),
            "Client B should receive UpdateNotification for directory"
        );

        let bytes = extract_notification_bytes(&notification.unwrap()).unwrap();
        let updated: DirectoryState = serde_json::from_slice(&bytes).unwrap();
        assert!(
            updated.entries.values().any(|e| e.name == "Test Farm"),
            "Notification should contain 'Test Farm' entry"
        );
    }
    println!("   PASSED");

    // ═══════════════════════════════════════════════════════════════════
    // Step 2: Storefront subscribe → add product → notification
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 2: storefront_subscribe_notifies_on_product_add ──");
    {
        let mut client_a = connect_to_node().await;
        let mut client_b = connect_to_node().await;

        let (supplier_id, vk) = make_dummy_supplier("Notify Farm");
        let (sf_contract, sf_key) = make_storefront_contract(&vk);

        let initial_sf = StorefrontState {
            info: cream_common::storefront::StorefrontInfo {
                owner: supplier_id,
                name: "Notify Farm".to_string(),
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

        let put_resp = recv_matching(&mut client_a, is_put_response, TIMEOUT).await;
        assert!(put_resp.is_some(), "Expected PutResponse for storefront");

        client_b
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *sf_key.id(),
                summary: None,
            }))
            .await
            .unwrap();

        let sub_resp = recv_matching(&mut client_b, is_subscribe_success, TIMEOUT).await;
        assert!(
            sub_resp.is_some(),
            "Expected SubscribeResponse for storefront"
        );

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

        let notification = recv_matching(&mut client_b, is_update_notification, TIMEOUT).await;
        assert!(
            notification.is_some(),
            "Client B should receive UpdateNotification for storefront product add"
        );

        let bytes = extract_notification_bytes(&notification.unwrap()).unwrap();
        let sf: StorefrontState = serde_json::from_slice(&bytes).unwrap();
        assert!(
            sf.products.values().any(|sp| sp.product.name == "Raw Milk"),
            "Notification should contain 'Raw Milk' product"
        );
    }
    println!("   PASSED");

    // ═══════════════════════════════════════════════════════════════════
    // Step 3: GET subscribe flag vs explicit Subscribe
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 3: get_subscribe_flag_vs_explicit_subscribe ──");
    {
        let mut client_a = connect_to_node().await;
        let mut client_b_get = connect_to_node().await;
        let mut client_b_sub = connect_to_node().await;

        let (supplier_id, vk) = make_dummy_supplier("Diagnostic Farm");
        let (sf_contract, sf_key) = make_storefront_contract(&vk);

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

        client_b_get
            .send(ClientRequest::ContractOp(ContractRequest::Get {
                key: *sf_key.id(),
                return_contract_code: false,
                subscribe: true,
                blocking_subscribe: false,
            }))
            .await
            .unwrap();

        recv_matching(&mut client_b_get, is_get_response, TIMEOUT)
            .await
            .expect("GetResponse for GET+subscribe");

        client_b_sub
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *sf_key.id(),
                summary: None,
            }))
            .await
            .unwrap();

        recv_matching(&mut client_b_sub, is_subscribe_success, TIMEOUT)
            .await
            .expect("SubscribeResponse for explicit Subscribe");

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

        let short_timeout = Duration::from_secs(3);

        let got_via_get_flag =
            recv_matching(&mut client_b_get, is_update_notification, short_timeout).await;
        let got_via_explicit_sub =
            recv_matching(&mut client_b_sub, is_update_notification, short_timeout).await;

        println!(
            "   GET with subscribe:true → notification received: {}",
            got_via_get_flag.is_some()
        );
        println!(
            "   Explicit Subscribe      → notification received: {}",
            got_via_explicit_sub.is_some()
        );

        assert!(
            got_via_get_flag.is_some() || got_via_explicit_sub.is_some(),
            "Neither subscription method produced an UpdateNotification"
        );
    }
    println!("   PASSED");

    // ═══════════════════════════════════════════════════════════════════
    // Step 4: Product count increments for subscriber (0 → 1 → 2)
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 4: product_count_increments_for_subscriber ──");
    {
        let mut supplier = connect_to_node().await;
        let mut customer = connect_to_node().await;

        let (supplier_id, vk) = make_dummy_supplier("Count Farm");
        let (sf_contract, sf_key) = make_storefront_contract(&vk);

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

        let initial_state = extract_get_response_state(&get_resp).expect("state bytes from GET");
        let sf: StorefrontState =
            serde_json::from_slice(&initial_state).expect("deserialize storefront from GET");
        assert_eq!(
            sf.products.len(),
            0,
            "Initial storefront should have 0 products"
        );

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

        // Add first product
        let product1 = make_dummy_product("Raw Milk");
        let mut sf_with_1 = initial_sf.clone();
        sf_with_1
            .products
            .insert(product1.product.id.clone(), product1);

        supplier
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: sf_key.clone(),
                data: UpdateData::State(State::from(serde_json::to_vec(&sf_with_1).unwrap())),
            }))
            .await
            .unwrap();

        let notif1 = recv_matching(&mut customer, is_update_notification, TIMEOUT)
            .await
            .expect("Notification after first product add");

        let bytes1 = extract_notification_bytes(&notif1).expect("notification bytes");
        let sf_notif1: StorefrontState = serde_json::from_slice(&bytes1).unwrap();
        assert_eq!(
            sf_notif1.products.len(),
            1,
            "After first product add, subscriber should see 1 product"
        );

        // Add second product
        tokio::time::sleep(Duration::from_millis(10)).await;
        let product2 = make_dummy_product("Aged Cheddar");
        let mut sf_with_2 = sf_with_1.clone();
        sf_with_2
            .products
            .insert(product2.product.id.clone(), product2);

        supplier
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: sf_key,
                data: UpdateData::State(State::from(serde_json::to_vec(&sf_with_2).unwrap())),
            }))
            .await
            .unwrap();

        let notif2 = recv_matching(&mut customer, is_update_notification, TIMEOUT)
            .await
            .expect("Notification after second product add");

        let bytes2 = extract_notification_bytes(&notif2).expect("notification bytes");
        let sf_notif2: StorefrontState = serde_json::from_slice(&bytes2).unwrap();
        assert_eq!(
            sf_notif2.products.len(),
            2,
            "After second product add, subscriber should see 2 products"
        );
    }
    println!("   PASSED");

    // ═══════════════════════════════════════════════════════════════════
    // Step 5: Harness — 3 suppliers with products (establishes fixture)
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 5: harness — 3 suppliers, products, multi-customer ──");
    {
        let mut h = TestHarness::setup().await;

        // Scenario 5a: product count increments for subscriber
        h.alice.subscribe_to_storefront(&h.gary).await;

        h.gary
            .add_product("Raw Milk", ProductCategory::Milk, 500)
            .await;
        let sf = h.alice.recv_storefront_update().await;
        assert_eq!(sf.products.len(), 1, "5a: 1 product after first add");

        h.gary
            .add_product("Aged Cheddar", ProductCategory::Cheese, 1200)
            .await;
        let sf = h.alice.recv_storefront_update().await;
        assert_eq!(sf.products.len(), 2, "5a: 2 products after second add");

        // Scenario 5b: independent storefronts
        h.alice.subscribe_to_storefront(&h.emma).await;

        h.emma
            .add_product("Artisan Butter", ProductCategory::Butter, 800)
            .await;

        let sf_emma = h.alice.recv_storefront_update().await;
        assert_eq!(sf_emma.products.len(), 1, "5b: Emma has 1 product");

        // Scenario 5c: two customers both see the same update
        h.bob.subscribe_to_storefront(&h.gary).await;

        h.gary
            .add_product("Kefir", ProductCategory::Kefir, 600)
            .await;

        let alice_sf = h.alice.recv_storefront_update().await;
        let bob_sf = h.bob.recv_storefront_update().await;
        assert_eq!(alice_sf.products.len(), 3, "5c: Alice sees 3 products");
        assert_eq!(bob_sf.products.len(), 3, "5c: Bob sees 3 products");
    }
    println!("   PASSED");

    println!("\n══ All node-integration steps passed ══");
}
