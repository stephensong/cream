use dioxus::prelude::*;

use cream_common::postcode::{distance_between_postcodes, format_postcode};

use super::app::Route;
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

/// A supplier entry for display in the directory.
#[derive(Clone, Debug)]
struct SupplierEntry {
    name: String,
    description: String,
    postcode: String,
    locality: Option<String>,
    distance_km: Option<f64>,
    product_count: usize,
}

#[component]
pub fn DirectoryView() -> Element {
    let user_state = use_user_state();
    let shared_state = use_shared_state();
    let mut search_query = use_signal(String::new);

    let state = user_state.read();
    let user_postcode = state.postcode.clone().unwrap_or_default();

    // Build supplier list from multiple sources
    let mut suppliers: Vec<SupplierEntry> = Vec::new();

    // Add current user if they're a supplier
    if state.is_supplier {
        let moniker = state.moniker.clone().unwrap_or_default();
        let desc = state
            .supplier_description
            .clone()
            .unwrap_or("Local supplier".into());
        let postcode = state.postcode.clone().unwrap_or_default();
        // Use product count from the network storefront, not local UserState
        let product_count = shared_state
            .read()
            .storefronts
            .get(&moniker)
            .map(|sf| sf.products.len())
            .unwrap_or(0);
        let locality = state.locality.clone();
        suppliers.push(SupplierEntry {
            name: moniker,
            description: desc,
            postcode: postcode.clone(),
            locality,
            distance_km: Some(0.0),
            product_count,
        });
    }
    drop(state);

    // Add suppliers from the Freenet directory (SharedState)
    // Determine our own SupplierId so we can skip our own directory entry
    // (we already added ourselves from local state above).
    let my_supplier_id = {
        let km_signal: Signal<Option<crate::components::key_manager::KeyManager>> = use_context();
        let km_guard = km_signal.read();
        km_guard.as_ref().map(|km| km.supplier_id())
    };
    {
        let shared = shared_state.read();
        #[cfg(target_family = "wasm")]
        {
            web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(
                &format!("[CREAM] DirectoryView render: {} directory entries, {} storefronts cached",
                    shared.directory.entries.len(), shared.storefronts.len())
            ));
            // Dump all storefront keys and their product counts
            for (sf_name, sf) in &shared.storefronts {
                web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(
                    &format!("[CREAM]   storefronts[\"{}\"] = {} products (info.name=\"{}\")",
                        sf_name, sf.products.len(), sf.info.name)
                ));
            }
        }
        for entry in shared.directory.entries.values() {
            // Skip our own entry (already added from local state above)
            if let Some(ref my_id) = my_supplier_id {
                if &entry.supplier == my_id {
                    continue;
                }
            }

            let postcode = entry.postcode.clone().unwrap_or_default();
            let dist = distance_between_postcodes(&user_postcode, &postcode);
            let product_count = shared
                .storefronts
                .get(&entry.name)
                .map(|sf| sf.products.len())
                .unwrap_or(0);

            #[cfg(target_family = "wasm")]
            web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(
                &format!("[CREAM] DirectoryView: {} → {} products", entry.name, product_count)
            ));

            suppliers.push(SupplierEntry {
                name: entry.name.clone(),
                description: entry.description.clone(),
                postcode,
                locality: entry.locality.clone(),
                distance_km: dist,
                product_count,
            });
        }
    }

    // Sort by distance (closest first), unknowns at the end
    suppliers.sort_by(|a, b| {
        let da = a.distance_km.unwrap_or(f64::MAX);
        let db = b.distance_km.unwrap_or(f64::MAX);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Filter by search query
    let query = search_query.read().to_lowercase();
    let filtered: Vec<_> = suppliers
        .into_iter()
        .filter(|s| {
            query.is_empty()
                || s.name.to_lowercase().contains(&query)
                || s.description.to_lowercase().contains(&query)
                || s.postcode.contains(&query)
        })
        .collect();

    rsx! {
        div { class: "directory-view",
            h2 { "Supplier Directory" }

            // Show connection status when use-node is active
            if cfg!(feature = "use-node") {
                {
                    let shared = shared_state.read();
                    if shared.connected {
                        rsx! { p { class: "connection-status connected", "Connected to Freenet" } }
                    } else if let Some(err) = &shared.last_error {
                        rsx! { p { class: "connection-status error", "Error: {err}" } }
                    } else {
                        rsx! { p { class: "connection-status connecting", "Connecting..." } }
                    }
                }
            }

            div { class: "search-bar",
                input {
                    r#type: "text",
                    placeholder: "Search suppliers...",
                    value: "{search_query}",
                    oninput: move |evt| search_query.set(evt.value()),
                }
            }
            div { class: "supplier-list",
                if filtered.is_empty() {
                    p { class: "empty-state", "No suppliers found." }
                } else {
                    {filtered.into_iter().map(|supplier| {
                        let distance_text = match supplier.distance_km {
                            Some(d) if d < 1.0 => "< 1 km away".to_string(),
                            Some(d) => format!("{:.0} km away", d),
                            None => "Distance unknown".to_string(),
                        };
                        rsx! {
                            div { class: "supplier-card",
                                key: "{supplier.name}",
                                h3 { "{supplier.name}" }
                                p { "{supplier.description}" }
                                {
                                    let location_name = format_postcode(&supplier.postcode, supplier.locality.as_deref());
                                    rsx! { p { class: "location", "{location_name} - {distance_text}" } }
                                }
                                p { class: "product-count", "{supplier.product_count} products" }
                                Link {
                                    to: Route::Supplier { name: supplier.name.clone() },
                                    "View Storefront"
                                }
                            }
                        }
                    })}
                }
            }
        }
    }
}

