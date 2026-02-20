#![cfg(feature = "node-tests")]

use cream_common::product::ProductCategory;
use cream_node_integration::harness::TestHarness;

/// Single harness setup, three scenarios tested sequentially.
///
/// We intentionally share one TestHarness so Gary, Emma, and Iris are only
/// registered in the directory once (avoiding duplicate entries from
/// independent setup() calls with random keys).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn harness_scenarios() {
    let mut h = TestHarness::setup().await;

    // ── Scenario 1: product count increments for subscriber ─────────
    h.alice.subscribe_to_storefront(&h.gary).await;

    h.gary
        .add_product("Raw Milk", ProductCategory::Milk, 500)
        .await;
    let sf = h.alice.recv_storefront_update().await;
    assert_eq!(sf.products.len(), 1, "scenario 1: 1 product after first add");

    h.gary
        .add_product("Aged Cheddar", ProductCategory::Cheese, 1200)
        .await;
    let sf = h.alice.recv_storefront_update().await;
    assert_eq!(sf.products.len(), 2, "scenario 1: 2 products after second add");

    // ── Scenario 2: independent storefronts ─────────────────────────
    h.alice.subscribe_to_storefront(&h.emma).await;

    h.emma
        .add_product("Artisan Butter", ProductCategory::Butter, 800)
        .await;

    // Alice sees Emma's storefront with 1 product (independent of Gary's 2)
    let sf_emma = h.alice.recv_storefront_update().await;
    assert_eq!(sf_emma.products.len(), 1, "scenario 2: Emma has 1 product");

    // ── Scenario 3: two customers both see the same update ──────────
    h.bob.subscribe_to_storefront(&h.gary).await;

    h.gary
        .add_product("Kefir", ProductCategory::Kefir, 600)
        .await;

    // Both Alice and Bob should see Gary's storefront now at 3 products
    let alice_sf = h.alice.recv_storefront_update().await;
    let bob_sf = h.bob.recv_storefront_update().await;
    assert_eq!(alice_sf.products.len(), 3, "scenario 3: Alice sees 3 products");
    assert_eq!(bob_sf.products.len(), 3, "scenario 3: Bob sees 3 products");
}
