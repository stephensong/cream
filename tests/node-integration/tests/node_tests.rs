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
    make_directory_entry, make_dummy_order, make_dummy_product, make_dummy_user,
    make_storefront_contract, node_url, recv_matching, wait_for_get, wait_for_put,
};

const TIMEOUT: Duration = Duration::from_secs(60);

/// Run a step with pre/post invariant checks (design-by-contract).
/// Steps 1-4 run before the harness exists and don't use this macro.
macro_rules! run_step {
    ($step:expr, $name:expr, $harness:expr, $body:block) => {{
        println!("\n=== Step {}: {} ===", $step, $name);
        $harness.check_invariants($step, "pre").await;
        $body
        $harness.check_invariants($step, "post").await;
        println!("=== Step {} PASSED ===", $step);
    }};
}

/// Variant for step 5 which creates the harness — only runs post-check.
macro_rules! run_step_post_only {
    ($step:expr, $name:expr, $harness:expr, $body:block) => {{
        println!("\n=== Step {}: {} ===", $step, $name);
        $body
        $harness.check_invariants($step, "post").await;
        println!("=== Step {} PASSED ===", $step);
    }};
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cumulative_node_tests() {
    tracing_subscriber::fmt::try_init().ok();

    // ═══════════════════════════════════════════════════════════════════
    // Step 1: Directory subscribe → update → notification
    // ═══════════════════════════════════════════════════════════════════
    println!("── Step 1: directory_subscribe_notifies_on_update ──");
    {
        let mut client_a = connect_to_node_at(&node_url(3002)).await;
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

        let (supplier_id, vk) = make_dummy_user("Test Farm");
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
            None,
        );

        let mut entries = BTreeMap::new();
        entries.insert(supplier_id, entry);
        let delta = DirectoryState { entries, extra: Default::default() };
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
        let mut client_a = connect_to_node_at(&node_url(3002)).await;
        let mut client_b = connect_to_node_at(&node_url(3003)).await;

        let (supplier_id, vk) = make_dummy_user("Notify Farm");
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
                market_products: BTreeMap::new(),
                extra: Default::default(),
            },
            products: BTreeMap::new(),
            orders: BTreeMap::new(),
            extra: Default::default(),
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
        let mut client_a = connect_to_node_at(&node_url(3002)).await;
        let mut client_b_get = connect_to_node_at(&node_url(3003)).await;
        let mut client_b_sub = connect_to_node_at(&node_url(3003)).await;

        let (supplier_id, vk) = make_dummy_user("Diagnostic Farm");
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
                market_products: BTreeMap::new(),
                extra: Default::default(),
            },
            products: BTreeMap::new(),
            orders: BTreeMap::new(),
            extra: Default::default(),
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
        // Use node-1 (3002) for supplier — node-3 (3004) update propagation
        // is still flaky even on v0.1.163 (works ~70% of the time).
        let mut supplier = connect_to_node_at(&node_url(3002)).await;
        let mut customer = connect_to_node_at(&node_url(3003)).await;

        let (supplier_id, vk) = make_dummy_user("Count Farm");
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
                market_products: BTreeMap::new(),
                extra: Default::default(),
            },
            products: BTreeMap::new(),
            orders: BTreeMap::new(),
            extra: Default::default(),
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
    let mut h = TestHarness::setup().await;
    run_step_post_only!(5, "harness — 3 suppliers, products, multi-customer", h, {
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
    });

    // ═══════════════════════════════════════════════════════════════════
    // Step 6: Order expiry — backdated orders across all deposit tiers
    //
    // Reuses the harness from Step 5. Alice is already subscribed to
    // Gary's storefront (3 products from 5a/5c).
    // ═══════════════════════════════════════════════════════════════════
    run_step!(6, "order_expiry_across_deposit_tiers", h, {
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
    });

    // ═══════════════════════════════════════════════════════════════════
    // Step 7: Update opening hours schedule → subscriber notification
    //
    // Reuses the harness from Step 5. Alice is already subscribed to
    // Gary's storefront.
    // ═══════════════════════════════════════════════════════════════════
    run_step!(7, "update_schedule_notifies_subscriber", h, {
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
    });

    // ═══════════════════════════════════════════════════════════════════
    // Step 8: Insufficient balance rejects order
    //
    // Creates a customer with 0 balance and verifies that placing an
    // order is rejected without touching the supplier's storefront.
    // ═══════════════════════════════════════════════════════════════════
    run_step!(8, "insufficient_balance_rejects_order", h, {
        use cream_node_integration::harness::Customer;
        use cream_node_integration::make_dummy_user;

        let (zara_id, zara_vk) = make_dummy_user("Zara");
        let api_zara = connect_to_node_at(&node_url(3004)).await;

        let mut zara = Customer {
            name: "Zara".to_string(),
            id: zara_id.clone(),
            verifying_key: zara_vk,
            api: api_zara,
            balance: 0,
            user_contract_key: None,
            inbox_key: None,
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
    });

    // ═══════════════════════════════════════════════════════════════════
    // Step 9: Root balance accounting — verify double-entry integrity
    //
    // The root user started with 1,000,000 CURD and gave 10,000 each
    // to Alice and Bob during setup. Verify the debits.
    // ═══════════════════════════════════════════════════════════════════
    run_step!(9, "root_balance_accounting", h, {
        use cream_common::identity::ROOT_USER_NAME;
        use cream_common::user_contract::UserContractState;
        use cream_common::wallet::TransactionKind;

        // GET root contract state from gateway — retry until all 6 transfers
        // have propagated (Freenet eventual consistency means the last UPDATE
        // may not be visible immediately on a different node).
        let expected_balance = 1_000_000 - (6 * 10_000);
        let expected_ledger_len = 7; // genesis credit + 6 debits (Gary, Emma, Iris, Alice, Bob, root)
        let mut root_state: UserContractState;
        let mut converged = false;
        for attempt in 1..=10 {
            let mut probe_check = connect_to_node_at(&node_url(3002)).await;
            let root_bytes = cream_node_integration::wait_for_get(
                &mut probe_check,
                *h.root_contract_key.id(),
                TIMEOUT,
            )
            .await
            .expect("GET root contract");
            root_state = serde_json::from_slice(&root_bytes).expect("deserialize root contract");

            if root_state.balance_curds == expected_balance
                && root_state.ledger.len() == expected_ledger_len
            {
                converged = true;
                break;
            }
            if attempt < 10 {
                println!(
                    "  [RETRY {}/10] balance={} (want {}), ledger={} (want {}) — waiting 2s",
                    attempt, root_state.balance_curds, expected_balance,
                    root_state.ledger.len(), expected_ledger_len,
                );
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            } else {
                panic!(
                    "9: root balance did not converge after {} attempts: balance={} (want {}), ledger={} (want {})",
                    attempt, root_state.balance_curds, expected_balance,
                    root_state.ledger.len(), expected_ledger_len,
                );
            }
        }
        assert!(converged);

        // Re-fetch the converged state for the cross-check below
        let mut probe = connect_to_node_at(&node_url(3002)).await;
        let root_bytes = cream_node_integration::wait_for_get(
            &mut probe,
            *h.root_contract_key.id(),
            TIMEOUT,
        )
        .await
        .expect("GET root contract (final)");
        let root_state: UserContractState =
            serde_json::from_slice(&root_bytes).expect("deserialize root contract");

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
            } else if recipient_name == "root" {
                h.root_admin.user_contract_key.as_ref().expect("root should have a user contract")
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
    });

    // ═══════════════════════════════════════════════════════════════════
    // Step 10: Fulfill order → escrow settlement
    //
    // Alice places a fresh order on Gary's storefront. Gary fulfills it.
    // We manually settle escrow (root → Gary) and verify balances.
    // ═══════════════════════════════════════════════════════════════════
    run_step!(10, "fulfill_order_settles_escrow", h, {
        use cream_common::user_contract::UserContractState;
        use cream_common::wallet::{TransactionKind, WalletTransaction};

        // Snapshot root balance before this step
        let mut probe = connect_to_node_at(&node_url(3002)).await;
        let root_bytes_before = cream_node_integration::wait_for_get(
            &mut probe,
            *h.root_contract_key.id(),
            TIMEOUT,
        )
        .await
        .expect("GET root contract before fulfill");
        let root_before: UserContractState =
            serde_json::from_slice(&root_bytes_before).expect("deserialize root before");
        let root_balance_before = root_before.balance_curds;

        // Snapshot Gary's balance before
        let gary_uc_key = h.gary.user_contract_key.expect("Gary should have a user contract");
        let gary_bytes_before = cream_node_integration::wait_for_get(
            &mut probe,
            *gary_uc_key.id(),
            TIMEOUT,
        )
        .await
        .expect("GET Gary's user contract before fulfill");
        let gary_before: UserContractState =
            serde_json::from_slice(&gary_bytes_before).expect("deserialize Gary before");
        let gary_balance_before = gary_before.balance_curds;
        drop(probe);

        // Alice places an order on Gary's storefront (Reserve2Days, qty=1, price=500 → deposit=50)
        let product_id = h.gary.storefront.products.values()
            .find(|sp| sp.product.name == "Raw Milk")
            .expect("Gary should have Raw Milk")
            .product.id.clone();
        let now = chrono::Utc::now();
        let order = make_dummy_order(
            &product_id,
            &h.alice.id,
            DepositTier::Reserve2Days,
            1,
            500,
            now,
        );
        let order_id = order.id.0.clone();
        let deposit_amount = order.deposit_amount;
        assert_eq!(deposit_amount, 50, "10: deposit for Reserve2Days on 500 should be 50");

        // Alice places the order (deducts from her harness balance, pushes to Gary's storefront)
        h.alice.place_order(order, &mut h.gary).await.expect("10: Alice should afford the order");

        // Alice receives the storefront update with the new order.
        // She may receive a stale notification first (from earlier steps), so loop.
        let placed_order_status = loop {
            let sf = h.alice.recv_storefront_update().await;
            if let Some(o) = sf.orders.values().find(|o| o.id.0 == order_id) {
                break o.status.clone();
            }
        };
        assert!(
            matches!(placed_order_status, OrderStatus::Reserved { .. }),
            "10: order should be Reserved"
        );

        // Record deposit transfer: Alice → root (escrow)
        // Debit Alice's user contract
        let alice_uc_key = h.alice.user_contract_key.expect("Alice should have a user contract");
        let tx_ref = format!("escrow:{}:{}", now.timestamp_millis(), order_id);
        let now_str = now.to_rfc3339();

        let alice_debit = WalletTransaction {
            id: 0,
            kind: TransactionKind::Debit,
            amount: deposit_amount,
            description: format!("Order {} deposit", order_id),
            sender: "Alice".to_string(),
            receiver: cream_common::identity::ROOT_USER_NAME.to_string(),
            tx_ref: tx_ref.clone(),
            timestamp: now_str.clone(),
            lightning_payment_hash: None,
            extra: Default::default(),
        };

        let mut probe = connect_to_node_at(&node_url(3002)).await;
        let alice_bytes = cream_node_integration::wait_for_get(&mut probe, *alice_uc_key.id(), TIMEOUT)
            .await.expect("GET Alice user contract");
        let mut alice_state: UserContractState = serde_json::from_slice(&alice_bytes).unwrap();
        alice_state.ledger.push(alice_debit);
        alice_state.balance_curds = alice_state.derive_balance();
        alice_state.next_tx_id = alice_state.ledger.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        alice_state.updated_at = chrono::Utc::now();

        let alice_update_bytes = serde_json::to_vec(&alice_state).unwrap();
        probe.send(ClientRequest::ContractOp(ContractRequest::Update {
            key: alice_uc_key,
            data: UpdateData::State(State::from(alice_update_bytes)),
        })).await.unwrap();
        cream_node_integration::recv_matching(&mut probe, cream_node_integration::is_update_response, TIMEOUT)
            .await.expect("UpdateResponse for Alice debit");

        // Credit root (escrow receipt)
        let root_credit = WalletTransaction {
            id: 0,
            kind: TransactionKind::Credit,
            amount: deposit_amount,
            description: format!("Order {} deposit escrow", order_id),
            sender: "Alice".to_string(),
            receiver: cream_common::identity::ROOT_USER_NAME.to_string(),
            tx_ref: tx_ref.clone(),
            timestamp: now_str.clone(),
            lightning_payment_hash: None,
            extra: Default::default(),
        };

        let root_bytes = cream_node_integration::wait_for_get(&mut probe, *h.root_contract_key.id(), TIMEOUT)
            .await.expect("GET root for escrow credit");
        let mut root_state: UserContractState = serde_json::from_slice(&root_bytes).unwrap();
        root_state.ledger.push(root_credit);
        root_state.balance_curds = root_state.derive_balance();
        root_state.next_tx_id = root_state.ledger.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        root_state.updated_at = chrono::Utc::now();
        root_state.signature = cream_common::identity::root_sign(&root_state.signable_bytes());

        let root_update_bytes = serde_json::to_vec(&root_state).unwrap();
        probe.send(ClientRequest::ContractOp(ContractRequest::Update {
            key: h.root_contract_key,
            data: UpdateData::State(State::from(root_update_bytes)),
        })).await.unwrap();
        cream_node_integration::recv_matching(&mut probe, cream_node_integration::is_update_response, TIMEOUT)
            .await.expect("UpdateResponse for root escrow credit");

        // Gary fulfills the order
        h.gary.fulfill_order(&order_id).await;

        // Alice receives the fulfilled notification (may get stale notifications first)
        loop {
            let sf = h.alice.recv_storefront_update().await;
            if let Some(o) = sf.orders.values().find(|o| o.id.0 == order_id) {
                if o.status == OrderStatus::Fulfilled {
                    break;
                }
            }
        }

        // Settle escrow: root → Gary (debit root, credit Gary)
        let settle_tx_ref = format!("settle:{}:{}", chrono::Utc::now().timestamp_millis(), order_id);
        let settle_now_str = chrono::Utc::now().to_rfc3339();

        // Debit root
        let root_settle_debit = WalletTransaction {
            id: 0,
            kind: TransactionKind::Debit,
            amount: deposit_amount,
            description: format!("Escrow settlement for order {}", order_id),
            sender: cream_common::identity::ROOT_USER_NAME.to_string(),
            receiver: "Gary".to_string(),
            tx_ref: settle_tx_ref.clone(),
            timestamp: settle_now_str.clone(),
            lightning_payment_hash: None,
            extra: Default::default(),
        };

        let root_bytes = cream_node_integration::wait_for_get(&mut probe, *h.root_contract_key.id(), TIMEOUT)
            .await.expect("GET root for settlement debit");
        let mut root_state: UserContractState = serde_json::from_slice(&root_bytes).unwrap();
        root_state.ledger.push(root_settle_debit);
        root_state.balance_curds = root_state.derive_balance();
        root_state.next_tx_id = root_state.ledger.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        root_state.updated_at = chrono::Utc::now();
        root_state.signature = cream_common::identity::root_sign(&root_state.signable_bytes());

        let root_update_bytes = serde_json::to_vec(&root_state).unwrap();
        probe.send(ClientRequest::ContractOp(ContractRequest::Update {
            key: h.root_contract_key,
            data: UpdateData::State(State::from(root_update_bytes)),
        })).await.unwrap();
        cream_node_integration::recv_matching(&mut probe, cream_node_integration::is_update_response, TIMEOUT)
            .await.expect("UpdateResponse for root settlement debit");

        // Credit Gary
        let gary_settle_credit = WalletTransaction {
            id: 0,
            kind: TransactionKind::Credit,
            amount: deposit_amount,
            description: format!("Escrow settlement for order {}", order_id),
            sender: cream_common::identity::ROOT_USER_NAME.to_string(),
            receiver: "Gary".to_string(),
            tx_ref: settle_tx_ref.clone(),
            timestamp: settle_now_str.clone(),
            lightning_payment_hash: None,
            extra: Default::default(),
        };

        let gary_bytes = cream_node_integration::wait_for_get(&mut probe, *gary_uc_key.id(), TIMEOUT)
            .await.expect("GET Gary for settlement credit");
        let mut gary_state: UserContractState = serde_json::from_slice(&gary_bytes).unwrap();
        gary_state.ledger.push(gary_settle_credit);
        gary_state.balance_curds = gary_state.derive_balance();
        gary_state.next_tx_id = gary_state.ledger.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        gary_state.updated_at = chrono::Utc::now();

        let gary_update_bytes = serde_json::to_vec(&gary_state).unwrap();
        probe.send(ClientRequest::ContractOp(ContractRequest::Update {
            key: gary_uc_key,
            data: UpdateData::State(State::from(gary_update_bytes)),
        })).await.unwrap();
        cream_node_integration::recv_matching(&mut probe, cream_node_integration::is_update_response, TIMEOUT)
            .await.expect("UpdateResponse for Gary settlement credit");

        // Verify final balances.
        // Freenet may serve stale state from GET immediately after a successful
        // UpdateResponse on the same connection — the update is acknowledged but
        // not yet visible to reads. Retry with backoff.
        let mut gary_final: UserContractState;
        loop {
            let gary_final_bytes = cream_node_integration::wait_for_get(&mut probe, *gary_uc_key.id(), TIMEOUT)
                .await.expect("GET Gary final");
            gary_final = serde_json::from_slice(&gary_final_bytes).unwrap();
            if gary_final.balance_curds == gary_balance_before + deposit_amount {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // Same stale-GET issue for root balance
        let mut root_final: UserContractState;
        loop {
            let root_final_bytes = cream_node_integration::wait_for_get(&mut probe, *h.root_contract_key.id(), TIMEOUT)
                .await.expect("GET root final");
            root_final = serde_json::from_slice(&root_final_bytes).unwrap();
            if root_final.balance_curds == root_balance_before {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        drop(probe);
    });

    // ═══════════════════════════════════════════════════════════════════
    // Step 11: Inbox message — Gary sends DM to Emma, Emma receives it
    // ═══════════════════════════════════════════════════════════════════
    run_step!(11, "inbox_message_delivery", h, {
        use cream_common::inbox::{InboxMessage, InboxState, MessageKind};

        // Emma subscribes to her own inbox and waits for subscribe confirmation
        let emma_inbox_key = h.emma.inbox_key
            .expect("Emma should have inbox key from harness setup");

        h.emma.api
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *emma_inbox_key.id(),
                summary: None,
            }))
            .await
            .unwrap();
        recv_matching(&mut h.emma.api, |r| {
            matches!(r, freenet_stdlib::client_api::HostResponse::ContractResponse(
                freenet_stdlib::client_api::ContractResponse::SubscribeResponse { subscribed: true, .. }
            ))
        }, TIMEOUT)
            .await
            .expect("Emma subscribe to her inbox");

        // Gary GETs Emma's inbox to ensure his node has the contract
        let emma_inbox_bytes = wait_for_get(
            &mut h.gary.api,
            *emma_inbox_key.id(),
            TIMEOUT,
        )
        .await
        .expect("Gary should be able to GET Emma's inbox");

        let emma_inbox_state: InboxState =
            serde_json::from_slice(&emma_inbox_bytes).unwrap();
        assert!(emma_inbox_state.messages.is_empty(), "Emma's inbox should start empty");

        // Gary sends a message to Emma's inbox via Update
        let now = chrono::Utc::now();
        let msg_id: u64 = now.timestamp_millis() as u64;
        let message = InboxMessage {
            id: msg_id,
            kind: MessageKind::DirectMessage,
            from_name: "Gary".to_string(),
            from_key: None,
            body: "Hey Emma, got any milk?".to_string(),
            toll_paid: 10,
            created_at: now,
            extra: Default::default(),
        };

        // Inbox uses the same user key (unified identity)
        let (emma_user_id, _) = make_dummy_user("Emma");
        let update_state = InboxState {
            owner: emma_user_id,
            messages: std::iter::once((msg_id, message)).collect(),
            updated_at: now,
            extra: Default::default(),
        };
        let update_bytes = serde_json::to_vec(&update_state).unwrap();

        h.gary.api
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: emma_inbox_key,
                data: UpdateData::State(State::from(update_bytes)),
            }))
            .await
            .unwrap();

        // Gary should get an UpdateResponse
        recv_matching(
            &mut h.gary.api,
            cream_node_integration::is_update_response,
            TIMEOUT,
        )
        .await
        .expect("UpdateResponse for Gary's message to Emma's inbox");

        println!("   Gary sent message to Emma's inbox, waiting for notification...");

        // Emma should receive an UpdateNotification with the message
        let notif = recv_matching(
            &mut h.emma.api,
            cream_node_integration::is_update_notification,
            TIMEOUT,
        )
        .await
        .expect("Emma should receive UpdateNotification for inbox message");

        let notif_bytes = extract_notification_bytes(&notif)
            .expect("notification should have state bytes");
        let inbox_update: InboxState =
            serde_json::from_slice(&notif_bytes).unwrap();

        assert!(
            inbox_update.messages.contains_key(&msg_id),
            "Notification should contain the message Gary sent"
        );
        let received_msg = &inbox_update.messages[&msg_id];
        assert_eq!(received_msg.from_name, "Gary");
        assert_eq!(received_msg.body, "Hey Emma, got any milk?");
        println!("   Emma received Gary's message via inbox notification");
    });

    // ═══════════════════════════════════════════════════════════════════
    // Step 12: Market directory — deploy, update, cross-node propagation
    // ═══════════════════════════════════════════════════════════════════
    run_step!(12, "market_directory_propagation", h, {
        use cream_common::market::MarketDirectoryState;
        use cream_node_integration::{connect_to_node_at, node_url};

        // Use a fresh connection to node-2 (Bob's main API is backlogged with notifications)
        let url_n2 = node_url(3003);
        let mut mkt_probe = connect_to_node_at(&url_n2).await;

        // Verify market directory deployed by harness is readable from node-2 (cross-node)
        let mkt_bytes = wait_for_get(
            &mut mkt_probe,
            *h.market_directory_key.id(),
            TIMEOUT,
        )
        .await
        .expect("Market directory should propagate to node-2");

        let mkt_state: MarketDirectoryState = serde_json::from_slice(&mkt_bytes).unwrap();
        assert_eq!(mkt_state.entries.len(), 1, "Should have 1 market from harness setup");

        let market = mkt_state.entries.values().next().unwrap();
        assert_eq!(market.name, "Coffs Harbour Farmers Market");
        assert!(market.suppliers.contains_key("Gary"));
        assert!(market.suppliers.contains_key("Emma"));
        assert_eq!(market.suppliers.len(), 2);
        println!("   Market directory readable from node-2 with correct data");

        // Subscribe on the fresh connection
        mkt_probe
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *h.market_directory_key.id(),
                summary: None,
            }))
            .await
            .unwrap();
        recv_matching(&mut mkt_probe, is_subscribe_success, TIMEOUT)
            .await
            .expect("Fresh probe subscribes to market directory");

        // Gary updates the market (add Iris as supplier)
        let mut updated_entry = market.clone();
        updated_entry.suppliers.insert("Iris".to_string(), cream_common::market::SupplierStatus::Accepted);
        updated_entry.updated_at = chrono::Utc::now();

        let mut entries = BTreeMap::new();
        entries.insert(h.gary.id.clone(), updated_entry);
        let update = MarketDirectoryState { entries, extra: Default::default() };
        let update_bytes = serde_json::to_vec(&update).unwrap();

        h.gary.api
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: h.market_directory_key,
                data: UpdateData::Delta(StateDelta::from(update_bytes)),
            }))
            .await
            .unwrap();

        recv_matching(
            &mut h.gary.api,
            cream_node_integration::is_update_response,
            TIMEOUT,
        )
        .await
        .expect("UpdateResponse for market update");

        // Fresh probe should receive notification with the updated market
        let notif = recv_matching(
            &mut mkt_probe,
            cream_node_integration::is_update_notification,
            TIMEOUT,
        )
        .await
        .expect("Probe should receive market directory update notification");

        let notif_bytes = extract_notification_bytes(&notif)
            .expect("notification should have state bytes");
        let notif_state: MarketDirectoryState = serde_json::from_slice(&notif_bytes).unwrap();
        let updated_market = notif_state.entries.values().next().unwrap();
        assert!(
            updated_market.suppliers.contains_key("Iris"),
            "Updated market should include Iris"
        );
        assert_eq!(updated_market.suppliers.len(), 3);
        println!("   Probe received market update notification with Iris added");
    });

    println!("\n══ All node-integration steps passed ══");
}
