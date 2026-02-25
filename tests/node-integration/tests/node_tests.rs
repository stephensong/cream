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
use cream_common::order::{DepositTier, OrderStatus};
use cream_common::product::ProductCategory;
use cream_common::storefront::StorefrontState;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest};
use freenet_stdlib::prelude::*;

use cream_node_integration::harness::TestHarness;
use cream_node_integration::{
    connect_to_node_at, extract_get_response_state, extract_notification_bytes, is_get_response,
    is_put_response, is_subscribe_success, is_update_notification, make_directory_contract,
    make_directory_entry, make_dummy_order, make_dummy_product, make_dummy_supplier,
    make_storefront_contract, node_url, recv_matching, wait_for_get, wait_for_put,
};

const TIMEOUT: Duration = Duration::from_secs(60);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cumulative_node_tests() {
    tracing_subscriber::fmt::try_init().ok();

    // ═══════════════════════════════════════════════════════════════════
    // Step 1: Directory subscribe → update → notification
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 1: directory_subscribe_notifies_on_update ──");
    {
        let mut client_a = connect_to_node_at(&node_url(3001)).await;
        let mut client_b = connect_to_node_at(&node_url(3003)).await;

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
        let _put_resp = recv_matching(&mut client_a, is_put_response, Duration::from_secs(2)).await;

        // Wait for directory to propagate to node-2 before subscribing
        wait_for_get(&mut client_b, *dir_key.id(), TIMEOUT)
            .await
            .expect("Directory contract should propagate to node-2");

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
        let entry = make_directory_entry(
            &supplier_id,
            "Test Farm",
            "Test Farm's dairy",
            "2000",
            "Sydney",
            cream_common::location::GeoLocation::new(-33.87, 151.21),
            sf_key,
            None,
        );

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
        let mut client_a = connect_to_node_at(&node_url(3001)).await;
        let mut client_b = connect_to_node_at(&node_url(3003)).await;

        let (supplier_id, vk) = make_dummy_supplier("Notify Farm");
        let (sf_contract, sf_key) = make_storefront_contract(&vk);

        let initial_sf = StorefrontState {
            info: cream_common::storefront::StorefrontInfo {
                owner: supplier_id,
                name: "Notify Farm".to_string(),
                description: "A test farm".to_string(),
                location: cream_common::location::GeoLocation::new(-33.87, 151.21),
                schedule: None,
                timezone: None,
                phone: None,
                email: None,
                address: None,
            },
            products: BTreeMap::new(),
            orders: BTreeMap::new(),
            messages: BTreeMap::new(),
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

        // Wait for storefront to propagate to node-2 before subscribing
        wait_for_get(&mut client_b, *sf_key.id(), TIMEOUT)
            .await
            .expect("Storefront contract should propagate to node-2");

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
        let mut client_a = connect_to_node_at(&node_url(3001)).await;
        let mut client_b_get = connect_to_node_at(&node_url(3003)).await;
        let mut client_b_sub = connect_to_node_at(&node_url(3003)).await;

        let (supplier_id, vk) = make_dummy_supplier("Diagnostic Farm");
        let (sf_contract, sf_key) = make_storefront_contract(&vk);

        let initial_sf = StorefrontState {
            info: cream_common::storefront::StorefrontInfo {
                owner: supplier_id,
                name: "Diagnostic Farm".to_string(),
                description: "Testing subscription methods".to_string(),
                location: cream_common::location::GeoLocation::new(-33.87, 151.21),
                schedule: None,
                timezone: None,
                phone: None,
                email: None,
                address: None,
            },
            products: BTreeMap::new(),
            orders: BTreeMap::new(),
            messages: BTreeMap::new(),
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

        // Wait for storefront to propagate to node-2 before GET+subscribe
        {
            let mut probe = connect_to_node_at(&node_url(3003)).await;
            wait_for_get(&mut probe, *sf_key.id(), TIMEOUT)
                .await
                .expect("Storefront contract should propagate to node-2");
            drop(probe);
        }

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
        let mut supplier = connect_to_node_at(&node_url(3001)).await;
        let mut customer = connect_to_node_at(&node_url(3003)).await;

        let (supplier_id, vk) = make_dummy_supplier("Count Farm");
        let (sf_contract, sf_key) = make_storefront_contract(&vk);

        let initial_sf = StorefrontState {
            info: cream_common::storefront::StorefrontInfo {
                owner: supplier_id,
                name: "Count Farm".to_string(),
                description: "Testing product count updates".to_string(),
                location: cream_common::location::GeoLocation::new(-33.87, 151.21),
                schedule: None,
                timezone: None,
                phone: None,
                email: None,
                address: None,
            },
            products: BTreeMap::new(),
            orders: BTreeMap::new(),
            messages: BTreeMap::new(),
        };
        let state_bytes = serde_json::to_vec(&initial_sf).unwrap();

        wait_for_put(
            &mut supplier,
            sf_contract,
            WrappedState::new(state_bytes),
            TIMEOUT,
        )
        .await
        .expect("PutResponse for storefront");

        // Wait for storefront to propagate to node-2 before customer GETs
        wait_for_get(&mut customer, *sf_key.id(), TIMEOUT)
            .await
            .expect("Storefront contract should propagate to node-2");

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
    let mut h = TestHarness::setup().await;
    {
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

    // ═══════════════════════════════════════════════════════════════════
    // Step 6: Order expiry — backdated orders across all deposit tiers
    //
    // Reuses the harness from Step 5. Alice is already subscribed to
    // Gary's storefront (3 products from 5a/5c).
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 6: order_expiry_across_deposit_tiers ──");
    {
        // Gary adds a product with quantity 10 for the expiry test
        let product = h
            .gary
            .add_product("Expiry Test Milk", ProductCategory::Milk, 500)
            .await;
        let product_id = product.product.id.clone();

        // Alice receives the product-add notification (now 4 products total)
        let sf = h.alice.recv_storefront_update().await;
        assert_eq!(sf.products.len(), 4, "6: Alice sees 4 products");

        // Place 3 backdated orders (one per deposit tier) — all already expired.
        // Reserve2Days: created 3 days ago (expired 1 day ago)
        // Reserve1Week: created 8 days ago (expired 1 day ago)
        // FullPayment:  created 366 days ago (expired 1 day ago)
        let now = chrono::Utc::now();

        let order_2day = make_dummy_order(
            &product_id,
            &h.alice.id,
            DepositTier::Reserve2Days,
            2,
            500,
            now - chrono::Duration::days(3),
        );
        let order_1week = make_dummy_order(
            &product_id,
            &h.alice.id,
            DepositTier::Reserve1Week,
            3,
            500,
            now - chrono::Duration::days(8),
        );
        let order_full = make_dummy_order(
            &product_id,
            &h.alice.id,
            DepositTier::FullPayment,
            1,
            500,
            now - chrono::Duration::days(366),
        );

        // All three should be Reserved (with past expiry dates)
        assert!(
            matches!(order_2day.status, OrderStatus::Reserved { .. }),
            "2-day order should start as Reserved"
        );
        assert!(
            matches!(order_1week.status, OrderStatus::Reserved { .. }),
            "1-week order should start as Reserved"
        );
        assert!(
            matches!(order_full.status, OrderStatus::Reserved { .. }),
            "full-payment order should start as Reserved"
        );

        // Add orders to Gary's storefront
        h.gary.add_order(order_2day).await;
        let sf = h.alice.recv_storefront_update().await;
        assert_eq!(sf.orders.len(), 1, "6: 1 order after first add");

        h.gary.add_order(order_1week).await;
        let sf = h.alice.recv_storefront_update().await;
        assert_eq!(sf.orders.len(), 2, "6: 2 orders after second add");

        h.gary.add_order(order_full).await;
        let sf = h.alice.recv_storefront_update().await;
        assert_eq!(sf.orders.len(), 3, "6: 3 orders after third add");

        // Verify available quantity is reduced: 10 total - (2+3+1) reserved = 4
        assert_eq!(
            h.gary.storefront.available_quantity(&product_id),
            4,
            "6: available quantity should be 4 with 6 units reserved"
        );

        // Run expiry — all 3 orders should transition to Expired
        let changed = h.gary.expire_orders().await;
        assert!(changed, "6: expire_orders should return true");

        // Verify all orders are now Expired locally
        for order in h.gary.storefront.orders.values() {
            assert_eq!(
                order.status,
                OrderStatus::Expired,
                "6: order {} should be Expired, got {}",
                order.id.0,
                order.status
            );
        }

        // Available quantity should be fully restored (expired orders don't reserve)
        assert_eq!(
            h.gary.storefront.available_quantity(&product_id),
            10,
            "6: available quantity should be restored to 10 after expiry"
        );

        // Alice should receive the expired state via network notification
        let sf = h.alice.recv_storefront_update().await;
        assert_eq!(sf.orders.len(), 3, "6: Alice sees 3 orders");
        for order in sf.orders.values() {
            assert_eq!(
                order.status,
                OrderStatus::Expired,
                "6: Alice sees order {} as Expired",
                order.id.0,
            );
        }

        // Alice's view of available quantity should also show full stock
        assert_eq!(
            sf.available_quantity(&product_id),
            10,
            "6: Alice's available quantity should be 10 after expiry"
        );
    }
    println!("   PASSED");

    // ═══════════════════════════════════════════════════════════════════
    // Step 7: Update opening hours schedule → subscriber notification
    //
    // Reuses the harness from Step 5. Alice is already subscribed to
    // Gary's storefront.
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 7: update_schedule_notifies_subscriber ──");
    {
        use cream_common::storefront::WeeklySchedule;

        // Build a schedule: Mon–Fri 8:00–17:00, Sat 9:00–12:00
        let mut schedule = WeeklySchedule::new();
        for day in 0..5u8 {
            // 8:00 = slot 16, 17:00 = slot 34
            schedule.set_range(day, 16, 34, true);
        }
        // Saturday: 9:00 = slot 18, 12:00 = slot 24
        schedule.set_range(5, 18, 24, true);

        h.gary
            .update_schedule(schedule.clone(), "Australia/Sydney")
            .await;

        // Alice should receive the updated storefront with schedule
        let sf = h.alice.recv_storefront_update().await;

        let recv_schedule = sf
            .info
            .schedule
            .as_ref()
            .expect("7: storefront should have a schedule");
        assert_eq!(
            sf.info.timezone.as_deref(),
            Some("Australia/Sydney"),
            "7: timezone should be Australia/Sydney"
        );

        // Verify Mon–Fri ranges: each should have one range (16, 34)
        for day in 0..5u8 {
            let ranges = recv_schedule.get_ranges(day);
            assert_eq!(
                ranges,
                vec![(16, 34)],
                "7: day {} should have 8:00–17:00",
                day
            );
        }

        // Verify Saturday range: (18, 24)
        assert_eq!(
            recv_schedule.get_ranges(5),
            vec![(18, 24)],
            "7: Saturday should have 9:00–12:00"
        );

        // Verify Sunday is closed
        assert!(
            recv_schedule.get_ranges(6).is_empty(),
            "7: Sunday should be closed"
        );

        // Spot-check open/closed states
        assert!(recv_schedule.is_open(0, 16), "7: Mon 8:00 should be open");
        assert!(recv_schedule.is_open(0, 33), "7: Mon 16:30 should be open");
        assert!(
            !recv_schedule.is_open(0, 34),
            "7: Mon 17:00 should be closed"
        );
        assert!(
            !recv_schedule.is_open(0, 15),
            "7: Mon 7:30 should be closed"
        );
        assert!(recv_schedule.is_open(5, 18), "7: Sat 9:00 should be open");
        assert!(
            !recv_schedule.is_open(6, 18),
            "7: Sun 9:00 should be closed"
        );
    }
    println!("   PASSED");

    // ═══════════════════════════════════════════════════════════════════
    // Step 8: Insufficient balance rejects order
    //
    // Creates a customer with 0 balance and verifies that placing an
    // order is rejected without touching the supplier's storefront.
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 8: insufficient_balance_rejects_order ──");
    {
        use cream_node_integration::harness::Customer;
        use cream_node_integration::make_dummy_customer;

        let (zara_id, zara_vk) = make_dummy_customer("Zara");
        let api_zara = connect_to_node_at(&node_url(3001)).await;

        let mut zara = Customer {
            name: "Zara".to_string(),
            id: zara_id.clone(),
            verifying_key: zara_vk,
            api: api_zara,
            balance: 0,
            user_contract_key: None,
        };

        // Subscribe Zara to Gary's storefront
        zara.subscribe_to_storefront(&h.gary).await;

        // Count Gary's current orders
        let orders_before = h.gary.storefront.orders.len();

        // Build an order for Gary's Expiry Test Milk (price 500, 2-Day Reserve tier)
        let now = chrono::Utc::now();
        let order = make_dummy_order(
            &h.gary
                .storefront
                .products
                .values()
                .find(|sp| sp.product.name == "Expiry Test Milk")
                .expect("Gary should have Expiry Test Milk")
                .product
                .id,
            &zara_id,
            DepositTier::Reserve2Days,
            2,
            500,
            now,
        );

        // Attempt to place the order — should fail
        let result = zara.place_order(order, &mut h.gary).await;
        assert!(
            result.is_err(),
            "8: order should be rejected with 0 balance"
        );

        // Gary's storefront order count should be unchanged
        assert_eq!(
            h.gary.storefront.orders.len(),
            orders_before,
            "8: Gary's order count should be unchanged"
        );

        // Zara's balance should still be 0
        assert_eq!(zara.balance, 0, "8: Zara's balance should still be 0");
    }
    println!("   PASSED");

    // ═══════════════════════════════════════════════════════════════════
    // Step 9: Root balance accounting — verify double-entry integrity
    //
    // The root user started with 1,000,000 CURD and gave 10,000 each
    // to Alice and Bob during setup. Verify the debits.
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 9: root_balance_accounting ──");
    {
        use cream_common::identity::ROOT_USER_NAME;
        use cream_common::user_contract::UserContractState;
        use cream_common::wallet::TransactionKind;

        // GET root contract state from gateway
        let mut probe = connect_to_node_at(&node_url(3001)).await;
        let root_bytes = cream_node_integration::wait_for_get(
            &mut probe,
            *h.root_contract_key.id(),
            TIMEOUT,
        )
        .await
        .expect("GET root contract");
        let root_state: UserContractState =
            serde_json::from_slice(&root_bytes).expect("deserialize root contract");

        // Root should have 1M - 5*10K = 950,000 CURD (3 suppliers + 2 customers)
        let expected_balance = 1_000_000 - (5 * 10_000);
        assert_eq!(
            root_state.balance_curds, expected_balance,
            "9: root balance should be {} (1M - 5*10K), got {}",
            expected_balance, root_state.balance_curds,
        );

        // Verify root has genesis credit + 5 debits = 6 ledger entries
        assert_eq!(
            root_state.ledger.len(),
            6,
            "9: root should have 6 ledger entries (genesis + 5 debits), got {}",
            root_state.ledger.len()
        );

        // Verify each debit has a matching credit on the recipient's user contract
        for debit in root_state.ledger.iter().filter(|t| t.kind == TransactionKind::Debit) {
            let recipient_name = &debit.receiver;

            // Find the recipient's user contract key
            let recipient_key = if recipient_name == "Alice" {
                h.alice.user_contract_key.as_ref().expect("Alice should have a user contract")
            } else if recipient_name == "Bob" {
                h.bob.user_contract_key.as_ref().expect("Bob should have a user contract")
            } else if recipient_name == "Gary" {
                h.gary.user_contract_key.as_ref().expect("Gary should have a user contract")
            } else if recipient_name == "Emma" {
                h.emma.user_contract_key.as_ref().expect("Emma should have a user contract")
            } else if recipient_name == "Iris" {
                h.iris.user_contract_key.as_ref().expect("Iris should have a user contract")
            } else {
                panic!("9: unexpected debit receiver: {}", recipient_name);
            };

            let recipient_bytes = cream_node_integration::wait_for_get(
                &mut probe,
                *recipient_key.id(),
                TIMEOUT,
            )
            .await
            .unwrap_or_else(|| panic!("GET user contract for {}", recipient_name));
            let recipient_state: UserContractState =
                serde_json::from_slice(&recipient_bytes)
                    .unwrap_or_else(|e| panic!("deserialize {} user contract: {}", recipient_name, e));

            // Find the matching credit by tx_ref
            let matching_credit = recipient_state.ledger.iter().find(|t| {
                t.tx_ref == debit.tx_ref && t.kind == TransactionKind::Credit
            });

            assert!(
                matching_credit.is_some(),
                "9: no matching credit on {}'s contract for tx_ref={}",
                recipient_name, debit.tx_ref,
            );

            let credit = matching_credit.unwrap();
            assert_eq!(credit.amount, debit.amount, "9: credit/debit amounts should match");
            assert_eq!(credit.sender, ROOT_USER_NAME, "9: credit sender should be root");
            assert_eq!(&credit.receiver, recipient_name, "9: credit receiver should be {}", recipient_name);
        }

        drop(probe);
    }
    println!("   PASSED");

    println!("\n══ All node-integration steps passed ══");
}
