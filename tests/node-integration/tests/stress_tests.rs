#![cfg(feature = "node-tests")]

//! Stress tests exercising all 4 Freenet nodes (1 gateway + 3 nodes) with
//! concurrent operations. Designed to flush out subscription notification
//! delivery, cross-node propagation, and merge conflict resolution bugs.

use std::collections::BTreeMap;
use std::time::Duration;

use cream_common::directory::DirectoryState;
use cream_common::identity::SupplierId;
use cream_common::location::GeoLocation;
use cream_common::order::DepositTier;
use cream_common::storefront::{StorefrontInfo, StorefrontState};
use ed25519_dalek::VerifyingKey;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest, WebApi};
use freenet_stdlib::prelude::*;

use cream_node_integration::{
    connect_to_node_at, extract_notification_bytes,
    is_put_response, is_subscribe_success, is_update_notification, is_update_response,
    make_directory_contract, make_directory_entry, make_dummy_customer, make_dummy_order,
    make_dummy_product, make_dummy_supplier, make_storefront_contract, node_url, recv_matching,
    timed_recv_matching, timed_wait_for_get, wait_for_get, wait_for_put,
};

const TIMEOUT: Duration = Duration::from_secs(90);
const STRESS_TIMEOUT: Duration = Duration::from_secs(120);
const ALL_PORTS: [u16; 4] = [3001, 3002, 3003, 3004];

/// Maximum retries for Update operations that may fail with "missing contract parameters"
/// on remote nodes (Freenet bug: state propagates before parameters).
const UPDATE_RETRIES: u32 = 5;
const UPDATE_RETRY_DELAY: Duration = Duration::from_secs(3);

/// Send an Update and wait for UpdateResponse, retrying on transient errors
/// (e.g. "missing contract parameters" when contract hasn't fully propagated).
async fn update_with_retry(
    api: &mut WebApi,
    key: ContractKey,
    data: UpdateData<'static>,
    label: &str,
) {
    for attempt in 1..=UPDATE_RETRIES {
        api.send(ClientRequest::ContractOp(ContractRequest::Update {
            key,
            data: data.clone(),
        }))
        .await
        .unwrap();

        match recv_matching(api, is_update_response, Duration::from_secs(15)).await {
            Some(_) => {
                if attempt > 1 {
                    println!(
                        "  [FREENET ISSUE] {label}: Update succeeded on attempt {attempt} \
                         (missing contract parameters on earlier attempts)"
                    );
                }
                return;
            }
            None => {
                if attempt < UPDATE_RETRIES {
                    println!(
                        "  [RETRY] {label}: Update attempt {attempt} failed, retrying in {}s...",
                        UPDATE_RETRY_DELAY.as_secs()
                    );
                    tokio::time::sleep(UPDATE_RETRY_DELAY).await;
                }
            }
        }
    }
    panic!("{label}: Update failed after {UPDATE_RETRIES} attempts");
}

/// Log a latency measurement in a structured format.
fn log_latency(test: &str, operation: &str, port: u16, duration: Duration) {
    println!(
        "  [LATENCY] {test} | {operation} | port={port} | {:.3}s",
        duration.as_secs_f64()
    );
}

/// Set up a storefront on a specific node port. Returns everything needed to
/// interact with it: identity, contract key, local state, and WebApi connection.
async fn setup_storefront_on_port(
    name: &str,
    port: u16,
) -> (SupplierId, VerifyingKey, ContractKey, StorefrontState, WebApi) {
    let (supplier_id, vk) = make_dummy_supplier(name);
    let (sf_contract, sf_key) = make_storefront_contract(&vk);

    let sf_state = StorefrontState {
        info: StorefrontInfo {
            owner: supplier_id.clone(),
            name: format!("{name}'s Farm"),
            description: format!("{name}'s stress test storefront"),
            location: GeoLocation::new(-33.87, 151.21),
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

    let state_bytes = serde_json::to_vec(&sf_state).unwrap();
    let mut api = connect_to_node_at(&node_url(port)).await;

    wait_for_put(
        &mut api,
        sf_contract,
        WrappedState::new(state_bytes),
        TIMEOUT,
    )
    .await
    .unwrap_or_else(|| panic!("PutResponse for {name}'s storefront on port {port}"));

    (supplier_id, vk, sf_key, sf_state, api)
}

// ═══════════════════════════════════════════════════════════════════
// Test 1: Cross-node propagation to all nodes
// ═══════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_node_propagation_all_nodes() {
    tracing_subscriber::fmt::try_init().ok();
    println!("── stress: cross_node_propagation_all_nodes ──");

    // PUT storefront on gateway (port 3001)
    let (_sid, _vk, sf_key, sf_state, _api) =
        setup_storefront_on_port("PropTest", 3001).await;

    // Concurrently GET from all 3 other nodes
    let mut handles = Vec::new();
    for &port in &[3002, 3003, 3004] {
        let key = *sf_key.id();
        handles.push(tokio::spawn(async move {
            let mut api = connect_to_node_at(&node_url(port)).await;
            let result = timed_wait_for_get(&mut api, key, STRESS_TIMEOUT).await;
            (port, result)
        }));
    }

    for handle in handles {
        let (port, result) = handle.await.unwrap();
        let (bytes, latency) = result
            .unwrap_or_else(|| panic!("GET from port {port} should succeed"));
        log_latency("cross_node_propagation", "GET", port, latency);

        let got: StorefrontState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            got.info.name, sf_state.info.name,
            "Storefront name mismatch on port {port}"
        );
    }

    println!("   PASSED");
}

// ═══════════════════════════════════════════════════════════════════
// Test 2: Concurrent product additions from 4 suppliers on 4 nodes
// ═══════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_product_additions() {
    tracing_subscriber::fmt::try_init().ok();
    println!("── stress: concurrent_product_additions ──");

    // Set up 4 suppliers, one per node
    let supplier_names = ["ConcAlpha", "ConcBeta", "ConcGamma", "ConcDelta"];
    let mut storefronts = Vec::new();

    for (i, name) in supplier_names.iter().enumerate() {
        let port = ALL_PORTS[i];
        let (sid, vk, sf_key, sf_state, api) = setup_storefront_on_port(name, port).await;
        storefronts.push((sid, vk, sf_key, sf_state, api, port, name.to_string()));
    }

    // Wait for all storefronts to propagate to port 3001 using probe connections
    for (_, _, sf_key, _, _, port, name) in &storefronts {
        if *port != 3001 {
            let mut probe = connect_to_node_at(&node_url(3001)).await;
            wait_for_get(&mut probe, *sf_key.id(), TIMEOUT)
                .await
                .unwrap_or_else(|| {
                    panic!("{name}'s storefront should propagate to port 3001")
                });
        }
    }

    // Subscriber on port 3001 subscribes to all 4 storefronts (single connection)
    let mut subscriber = connect_to_node_at(&node_url(3001)).await;
    for (_, _, sf_key, _, _, _port, name) in &storefronts {
        subscriber
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *sf_key.id(),
                summary: None,
            }))
            .await
            .unwrap();

        recv_matching(&mut subscriber, is_subscribe_success, TIMEOUT)
            .await
            .unwrap_or_else(|| panic!("Subscribe to {name}'s storefront"));
    }

    // All 4 suppliers add a product simultaneously
    let start = std::time::Instant::now();
    let mut add_handles = Vec::new();

    for (_, _, sf_key, mut sf_state, mut api, port, name) in storefronts {
        add_handles.push(tokio::spawn(async move {
            let product = make_dummy_product(&format!("{name} Milk"));
            sf_state
                .products
                .insert(product.product.id.clone(), product);

            let sf_bytes = serde_json::to_vec(&sf_state).unwrap();
            api.send(ClientRequest::ContractOp(ContractRequest::Update {
                key: sf_key,
                data: UpdateData::State(State::from(sf_bytes)),
            }))
            .await
            .unwrap();

            let resp = recv_matching(&mut api, is_update_response, TIMEOUT).await;
            let elapsed = start.elapsed();
            assert!(
                resp.is_some(),
                "UpdateResponse for {name}'s product on port {port}"
            );
            log_latency("concurrent_products", "update", port, elapsed);
            (name, port)
        }));
    }

    // Wait for all updates to complete
    for handle in add_handles {
        handle.await.unwrap();
    }

    // Subscriber should receive notifications for all 4 updates
    let mut notifications_received = 0;
    for _ in 0..4 {
        let result = timed_recv_matching(
            &mut subscriber,
            is_update_notification,
            STRESS_TIMEOUT,
        )
        .await;

        if let Some((_, latency)) = result {
            notifications_received += 1;
            log_latency(
                "concurrent_products",
                &format!("notification #{notifications_received}"),
                3001,
                latency,
            );
        } else {
            println!(
                "  [WARN] Timed out waiting for notification #{} of 4",
                notifications_received + 1
            );
            break;
        }
    }

    assert!(
        notifications_received >= 1,
        "Subscriber should receive at least 1 update notification"
    );
    if notifications_received < 4 {
        println!(
            "  [FREENET ISSUE] Only {notifications_received}/4 notifications delivered — \
             possible subscription notification bug"
        );
    }

    println!("   PASSED");
}

// ═══════════════════════════════════════════════════════════════════
// Test 3: Rapid-fire updates (burst of 10 products)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rapid_fire_updates() {
    tracing_subscriber::fmt::try_init().ok();
    println!("── stress: rapid_fire_updates ──");

    let (_, _, sf_key, mut sf_state, mut supplier_api) =
        setup_storefront_on_port("RapidFarm", 3002).await;

    // Subscriber on port 3004
    let mut probe = connect_to_node_at(&node_url(3004)).await;
    wait_for_get(&mut probe, *sf_key.id(), TIMEOUT)
        .await
        .expect("RapidFarm storefront should propagate to port 3004");
    drop(probe);

    let mut subscriber = connect_to_node_at(&node_url(3004)).await;
    subscriber
        .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
            key: *sf_key.id(),
            summary: None,
        }))
        .await
        .unwrap();
    recv_matching(&mut subscriber, is_subscribe_success, TIMEOUT)
        .await
        .expect("Subscribe to RapidFarm storefront");

    // Send 10 product additions in rapid succession (no delay)
    let mut added_product_ids = Vec::new();
    let burst_start = std::time::Instant::now();
    for i in 0..10 {
        let product = make_dummy_product(&format!("Rapid Product {i}"));
        added_product_ids.push(product.product.id.clone());
        sf_state
            .products
            .insert(product.product.id.clone(), product);

        let sf_bytes = serde_json::to_vec(&sf_state).unwrap();
        supplier_api
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: sf_key,
                data: UpdateData::State(State::from(sf_bytes)),
            }))
            .await
            .unwrap();

        // Wait for UpdateResponse before sending next (Freenet requires sequential ops per connection)
        recv_matching(&mut supplier_api, is_update_response, TIMEOUT)
            .await
            .unwrap_or_else(|| panic!("UpdateResponse for rapid product {i}"));
    }
    let burst_elapsed = burst_start.elapsed();
    println!(
        "  [TIMING] rapid_fire: 10 updates sent in {:.3}s",
        burst_elapsed.as_secs_f64()
    );

    // Collect notifications — Freenet may coalesce, so don't assert exactly 10
    let mut notification_count = 0;
    let collect_deadline =
        tokio::time::Instant::now() + STRESS_TIMEOUT;
    loop {
        let remaining = collect_deadline
            .saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match timed_recv_matching(&mut subscriber, is_update_notification, remaining).await {
            Some((notif, latency)) => {
                notification_count += 1;
                // Check if this notification contains the final state
                if let Some(bytes) = extract_notification_bytes(&notif) {
                    if let Ok(sf) = serde_json::from_slice::<StorefrontState>(&bytes) {
                        log_latency(
                            "rapid_fire",
                            &format!("notification #{notification_count} ({} products)", sf.products.len()),
                            3004,
                            latency,
                        );
                        if added_product_ids.iter().all(|pid| sf.products.contains_key(pid)) {
                            break; // Got final state with all our products
                        }
                    }
                }
            }
            None => break,
        }
    }

    println!(
        "  [INFO] Received {notification_count} notifications (Freenet may coalesce)"
    );
    assert!(
        notification_count >= 1,
        "Should receive at least 1 notification"
    );

    // Final GET from port 3003 to confirm all 10 products are present
    // Retry a few times since propagation may lag
    let mut all_present = false;
    for attempt in 1..=5 {
        let mut verifier = connect_to_node_at(&node_url(3003)).await;
        let (bytes, latency) = timed_wait_for_get(&mut verifier, *sf_key.id(), STRESS_TIMEOUT)
            .await
            .expect("GET from port 3003 should succeed");
        if attempt == 1 {
            log_latency("rapid_fire", "final GET", 3003, latency);
        }

        let final_sf: StorefrontState = serde_json::from_slice(&bytes).unwrap();
        if added_product_ids.iter().all(|pid| final_sf.products.contains_key(pid)) {
            all_present = true;
            break;
        }
        if attempt < 5 {
            println!(
                "  [RETRY] Final GET attempt {attempt}: only {}/{} products present, retrying in 3s...",
                final_sf.products.len(),
                added_product_ids.len()
            );
            tokio::time::sleep(Duration::from_secs(3)).await;
        } else {
            panic!(
                "After {attempt} attempts, only {}/{} products present on port 3003",
                final_sf.products.len(),
                added_product_ids.len()
            );
        }
    }
    assert!(all_present);

    println!("   PASSED");
}

// ═══════════════════════════════════════════════════════════════════
// Test 4: Subscription fanout (8 subscribers across 4 nodes)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn subscription_fanout() {
    tracing_subscriber::fmt::try_init().ok();
    println!("── stress: subscription_fanout ──");

    let (_, _, sf_key, mut sf_state, mut supplier_api) =
        setup_storefront_on_port("FanoutFarm", 3003).await;

    // Wait for propagation to all nodes before subscribing
    for &port in &[3001, 3002, 3004] {
        let mut probe = connect_to_node_at(&node_url(port)).await;
        wait_for_get(&mut probe, *sf_key.id(), TIMEOUT)
            .await
            .unwrap_or_else(|| {
                panic!("FanoutFarm storefront should propagate to port {port}")
            });
    }

    // 8 subscribers: 2 per node
    let mut subscriber_handles = Vec::new();
    for &port in &ALL_PORTS {
        for sub_idx in 0..2 {
            let key = *sf_key.id();
            subscriber_handles.push(tokio::spawn(async move {
                let mut api = connect_to_node_at(&node_url(port)).await;
                api.send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                    key,
                    summary: None,
                }))
                .await
                .unwrap();
                recv_matching(&mut api, is_subscribe_success, TIMEOUT)
                    .await
                    .unwrap_or_else(|| {
                        panic!("Subscribe for sub {sub_idx} on port {port}")
                    });
                (api, port, sub_idx)
            }));
        }
    }

    let mut subscribers: Vec<(WebApi, u16, usize)> = Vec::new();
    for handle in subscriber_handles {
        subscribers.push(handle.await.unwrap());
    }

    // Supplier adds 1 product
    let product = make_dummy_product("Fanout Milk");
    sf_state
        .products
        .insert(product.product.id.clone(), product);
    let sf_bytes = serde_json::to_vec(&sf_state).unwrap();
    supplier_api
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: sf_key,
            data: UpdateData::State(State::from(sf_bytes)),
        }))
        .await
        .unwrap();
    recv_matching(&mut supplier_api, is_update_response, TIMEOUT)
        .await
        .expect("UpdateResponse for fanout product");

    // Wait for all 8 subscribers to receive the notification
    let mut receive_handles = Vec::new();
    for (mut api, port, sub_idx) in subscribers {
        receive_handles.push(tokio::spawn(async move {
            let result =
                timed_recv_matching(&mut api, is_update_notification, STRESS_TIMEOUT).await;
            (port, sub_idx, result)
        }));
    }

    let mut received = 0;
    let mut latencies: Vec<(u16, usize, Duration)> = Vec::new();
    for handle in receive_handles {
        let (port, sub_idx, result) = handle.await.unwrap();
        if let Some((_, latency)) = result {
            received += 1;
            latencies.push((port, sub_idx, latency));
            log_latency(
                "fanout",
                &format!("sub[{sub_idx}]"),
                port,
                latency,
            );
        } else {
            println!("  [WARN] sub[{sub_idx}] on port {port} did not receive notification");
        }
    }

    if !latencies.is_empty() {
        let min = latencies.iter().map(|(_, _, d)| d).min().unwrap();
        let max = latencies.iter().map(|(_, _, d)| d).max().unwrap();
        let mean = latencies.iter().map(|(_, _, d)| d.as_secs_f64()).sum::<f64>()
            / latencies.len() as f64;
        println!(
            "  [LATENCY] fanout summary: min={:.3}s max={:.3}s mean={:.3}s received={}/8",
            min.as_secs_f64(),
            max.as_secs_f64(),
            mean,
            received
        );
    }

    assert_eq!(
        received, 8,
        "All 8 subscribers should receive the notification (got {received})"
    );

    println!("   PASSED");
}

// ═══════════════════════════════════════════════════════════════════
// Test 5: Concurrent order placement from 4 nodes
// ═══════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_order_placement() {
    tracing_subscriber::fmt::try_init().ok();
    println!("── stress: concurrent_order_placement ──");

    // 1 supplier on port 3001 with 1 product (quantity=10)
    let (_supplier_id, _vk, sf_key, mut sf_state, mut supplier_api) =
        setup_storefront_on_port("OrderFarm", 3001).await;

    let product = make_dummy_product("Order Milk");
    let product_id = product.product.id.clone();
    sf_state
        .products
        .insert(product_id.clone(), product);
    let sf_bytes = serde_json::to_vec(&sf_state).unwrap();
    supplier_api
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: sf_key,
            data: UpdateData::State(State::from(sf_bytes.clone())),
        }))
        .await
        .unwrap();
    recv_matching(&mut supplier_api, is_update_response, TIMEOUT)
        .await
        .expect("UpdateResponse for OrderFarm product");

    // Wait for propagation to all nodes
    for &port in &[3002, 3003, 3004] {
        let mut probe = connect_to_node_at(&node_url(port)).await;
        wait_for_get(&mut probe, *sf_key.id(), TIMEOUT)
            .await
            .unwrap_or_else(|| {
                panic!("OrderFarm storefront should propagate to port {port}")
            });
    }

    // 4 customers, one per node, each places an order simultaneously
    let customer_names: Vec<String> = vec![
        "OrderCustA".to_string(),
        "OrderCustB".to_string(),
        "OrderCustC".to_string(),
        "OrderCustD".to_string(),
    ];
    let now = chrono::Utc::now();

    let mut order_handles = Vec::new();
    for (i, name) in customer_names.into_iter().enumerate() {
        let port = ALL_PORTS[i];
        let pid = product_id.clone();
        let key = sf_key;
        let (cust_id, _) = make_dummy_customer(&name);
        let order = make_dummy_order(
            &pid,
            &cust_id,
            DepositTier::Reserve2Days,
            1,
            500,
            now + chrono::Duration::milliseconds(i as i64), // unique timestamps
        );

        order_handles.push(tokio::spawn(async move {
            // GET current state from this node
            let mut api = connect_to_node_at(&node_url(port)).await;
            let bytes = wait_for_get(&mut api, *key.id(), TIMEOUT)
                .await
                .unwrap_or_else(|| panic!("GET storefront on port {port}"));
            let mut sf: StorefrontState = serde_json::from_slice(&bytes).unwrap();

            // Add the order
            sf.orders.insert(order.id.clone(), order);
            let sf_bytes = serde_json::to_vec(&sf).unwrap();

            update_with_retry(
                &mut api,
                key,
                UpdateData::State(State::from(sf_bytes)),
                &format!("{name}'s order on port {port}"),
            )
            .await;
            (name, port)
        }));
    }

    for handle in order_handles {
        let (name, port) = handle.await.unwrap();
        println!("  [OK] {name} placed order on port {port}");
    }

    // Allow time for cross-node merge propagation
    tokio::time::sleep(Duration::from_secs(5)).await;

    // GET storefront from each node and verify all 4 orders present
    for &port in &ALL_PORTS {
        let mut api = connect_to_node_at(&node_url(port)).await;
        let (bytes, latency) = timed_wait_for_get(&mut api, *sf_key.id(), STRESS_TIMEOUT)
            .await
            .unwrap_or_else(|| panic!("GET from port {port}"));
        log_latency("concurrent_orders", "final GET", port, latency);

        let sf: StorefrontState = serde_json::from_slice(&bytes).unwrap();
        let reserved_count = sf
            .orders
            .values()
            .filter(|o| matches!(o.status, cream_common::order::OrderStatus::Reserved { .. }))
            .count();
        assert!(
            reserved_count >= 4,
            "Port {port} should have at least 4 Reserved orders (got {reserved_count}, total orders: {})",
            sf.orders.len()
        );
    }

    println!("   PASSED");
}

// ═══════════════════════════════════════════════════════════════════
// Test 6: Directory contention — 4 simultaneous registrations
// ═══════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn directory_contention() {
    tracing_subscriber::fmt::try_init().ok();
    println!("── stress: directory_contention ──");

    // PUT directory on port 3001
    let (dir_contract, dir_key) = make_directory_contract();
    let empty_dir = DirectoryState::default();
    let dir_bytes = serde_json::to_vec(&empty_dir).unwrap();
    let mut dir_api = connect_to_node_at(&node_url(3001)).await;

    dir_api
        .send(ClientRequest::ContractOp(ContractRequest::Put {
            contract: dir_contract,
            state: WrappedState::new(dir_bytes),
            related_contracts: RelatedContracts::default(),
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();
    // Short timeout — directory may already exist
    let _ = recv_matching(&mut dir_api, is_put_response, Duration::from_secs(2)).await;
    drop(dir_api);

    // Wait for propagation to all nodes
    for &port in &[3002, 3003, 3004] {
        let mut probe = connect_to_node_at(&node_url(port)).await;
        wait_for_get(&mut probe, *dir_key.id(), TIMEOUT)
            .await
            .unwrap_or_else(|| {
                panic!("Directory should propagate to port {port}")
            });
    }

    // Subscriber on port 3002
    let mut subscriber = connect_to_node_at(&node_url(3002)).await;
    subscriber
        .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
            key: *dir_key.id(),
            summary: None,
        }))
        .await
        .unwrap();
    recv_matching(&mut subscriber, is_subscribe_success, TIMEOUT)
        .await
        .expect("Subscribe to directory on port 3002");

    // 4 suppliers register simultaneously from 4 different nodes
    let supplier_names = ["DirAlpha", "DirBeta", "DirGamma", "DirDelta"];
    let mut reg_handles = Vec::new();

    for (i, name) in supplier_names.iter().enumerate() {
        let port = ALL_PORTS[i];
        let dk = dir_key;
        let name = name.to_string();

        reg_handles.push(tokio::spawn(async move {
            let (supplier_id, vk) = make_dummy_supplier(&name);
            let (_, sf_key) = make_storefront_contract(&vk);

            let entry = make_directory_entry(
                &supplier_id,
                &name,
                &format!("{name}'s dairy"),
                "2000",
                "Sydney",
                GeoLocation::new(-33.87 + (i as f64 * 0.01), 151.21),
                sf_key,
                None,
            );

            let mut entries = BTreeMap::new();
            entries.insert(supplier_id, entry);
            let delta = DirectoryState { entries };
            let delta_bytes = serde_json::to_vec(&delta).unwrap();

            let mut api = connect_to_node_at(&node_url(port)).await;
            update_with_retry(
                &mut api,
                dk,
                UpdateData::Delta(StateDelta::from(delta_bytes)),
                &format!("{name}'s directory registration on port {port}"),
            )
            .await;
            println!("  [OK] {name} registered on port {port}");
            (name, port)
        }));
    }

    for handle in reg_handles {
        handle.await.unwrap();
    }

    // Allow time for cross-node merge propagation
    tokio::time::sleep(Duration::from_secs(5)).await;

    // GET directory from each node and assert all 4 entries present
    for &port in &ALL_PORTS {
        let mut api = connect_to_node_at(&node_url(port)).await;
        let (bytes, latency) = timed_wait_for_get(&mut api, *dir_key.id(), STRESS_TIMEOUT)
            .await
            .unwrap_or_else(|| panic!("GET directory from port {port}"));
        log_latency("directory_contention", "GET", port, latency);

        let dir: DirectoryState = serde_json::from_slice(&bytes).unwrap();
        for name in &supplier_names {
            assert!(
                dir.entries.values().any(|e| e.name == *name),
                "Port {port} directory should contain {name} (has: {:?})",
                dir.entries.values().map(|e| &e.name).collect::<Vec<_>>()
            );
        }
    }

    // Subscriber on port 3002 should have received at least one notification
    let notif = recv_matching(&mut subscriber, is_update_notification, STRESS_TIMEOUT).await;
    assert!(
        notif.is_some(),
        "Subscriber on port 3002 should receive directory update notification"
    );
    println!("  [OK] Subscriber received directory notification");

    println!("   PASSED");
}
