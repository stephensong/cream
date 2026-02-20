#![cfg(feature = "node-tests")]

use cream_common::product::ProductCategory;
use cream_node_integration::harness::TestHarness;

/// Subscriber sees product count go from 0 → 1 → 2 as supplier adds products.
/// (Harness-based version of subscription_tests::product_count_increments_for_subscriber)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn product_count_increments_for_subscriber() {
    let mut h = TestHarness::setup().await;
    h.alice.subscribe_to_storefront(&h.gary).await;

    h.gary
        .add_product("Raw Milk", ProductCategory::Milk, 500)
        .await;
    let sf = h.alice.recv_storefront_update().await;
    assert_eq!(sf.products.len(), 1);

    h.gary
        .add_product("Aged Cheddar", ProductCategory::Cheese, 1200)
        .await;
    let sf = h.alice.recv_storefront_update().await;
    assert_eq!(sf.products.len(), 2);
}

/// Two suppliers' storefronts are independent — adding to one doesn't affect the other.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_suppliers_independent_storefronts() {
    let mut h = TestHarness::setup().await;
    h.alice.subscribe_to_storefront(&h.gary).await;
    h.alice.subscribe_to_storefront(&h.emma).await;

    h.gary
        .add_product("Raw Milk", ProductCategory::Milk, 500)
        .await;
    h.emma
        .add_product("Artisan Butter", ProductCategory::Butter, 800)
        .await;

    // Alice gets two updates (one per storefront) — each should have 1 product
    let sf1 = h.alice.recv_storefront_update().await;
    assert_eq!(sf1.products.len(), 1);
    let sf2 = h.alice.recv_storefront_update().await;
    assert_eq!(sf2.products.len(), 1);
}

/// Two customers both receive the same storefront update.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_customers_both_see_update() {
    let mut h = TestHarness::setup().await;
    h.alice.subscribe_to_storefront(&h.gary).await;
    h.bob.subscribe_to_storefront(&h.gary).await;

    h.gary
        .add_product("Kefir", ProductCategory::Kefir, 600)
        .await;

    let alice_sf = h.alice.recv_storefront_update().await;
    let bob_sf = h.bob.recv_storefront_update().await;
    assert_eq!(alice_sf.products.len(), 1);
    assert_eq!(bob_sf.products.len(), 1);
}
