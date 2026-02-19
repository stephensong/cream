use dioxus::prelude::*;

use cream_common::postcode::{distance_between_postcodes, format_postcode};

use super::shared_state::use_shared_state;
use super::storefront_view::StorefrontView;
use super::user_state::use_user_state;

/// A supplier entry for display in the directory.
#[derive(Clone, Debug)]
struct SupplierEntry {
    name: String,
    description: String,
    postcode: String,
    distance_km: Option<f64>,
    product_count: usize,
}

#[component]
pub fn DirectoryView() -> Element {
    let user_state = use_user_state();
    let shared_state = use_shared_state();
    let mut selected_supplier = use_signal(|| None::<String>);
    let mut search_query = use_signal(|| String::new());

    if let Some(supplier_name) = selected_supplier.read().clone() {
        return rsx! {
            button {
                onclick: move |_| selected_supplier.set(None),
                "Back to Directory"
            }
            StorefrontView { supplier_name }
        };
    }

    let state = user_state.read();
    let user_postcode = state.postcode.clone().unwrap_or_default();

    // Build supplier list from multiple sources
    let mut suppliers: Vec<SupplierEntry> = Vec::new();

    // Add current user if they're a supplier with products
    if state.is_supplier && !state.products.is_empty() {
        let moniker = state.moniker.clone().unwrap_or_default();
        let desc = state
            .supplier_description
            .clone()
            .unwrap_or("Local supplier".into());
        let postcode = state.postcode.clone().unwrap_or_default();
        suppliers.push(SupplierEntry {
            name: moniker,
            description: desc,
            postcode: postcode.clone(),
            distance_km: Some(0.0),
            product_count: state.products.len(),
        });
    }
    drop(state);

    // Add suppliers from the Freenet directory (SharedState)
    {
        let shared = shared_state.read();
        for entry in shared.directory.entries.values() {
            // Skip if this is our own entry (already added above)
            let user = user_state.read();
            if user.moniker.as_deref() == Some(&entry.name) {
                continue;
            }
            drop(user);

            // Use postcode from the geo-location (approximate, based on latitude)
            // For now, use a placeholder; in the real implementation the directory
            // entry would carry a postcode or we'd reverse-geocode.
            let postcode = format!(
                "{:.0}",
                entry.location.latitude.abs() * 100.0
            );
            let dist = distance_between_postcodes(&user_postcode, &postcode);
            let product_count = shared
                .storefronts
                .get(&entry.name)
                .map(|sf| sf.products.len())
                .unwrap_or(0);

            suppliers.push(SupplierEntry {
                name: entry.name.clone(),
                description: entry.description.clone(),
                postcode,
                distance_km: dist,
                product_count,
            });
        }
    }

    // Add example data when feature is enabled and not connected to node
    if cfg!(feature = "example-data") {
        let shared = shared_state.read();
        if shared.directory.entries.is_empty() {
            for (name, desc, postcode) in example_suppliers() {
                let dist = distance_between_postcodes(&user_postcode, &postcode);
                suppliers.push(SupplierEntry {
                    name,
                    description: desc,
                    postcode,
                    distance_km: dist,
                    product_count: 5,
                });
            }
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
                        let name_clone = supplier.name.clone();
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
                                    let location_name = format_postcode(&supplier.postcode);
                                    rsx! { p { class: "location", "{location_name} - {distance_text}" } }
                                }
                                p { class: "product-count", "{supplier.product_count} products" }
                                button {
                                    onclick: move |_| selected_supplier.set(Some(name_clone.clone())),
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

/// Example suppliers with Australian postcodes for development.
fn example_suppliers() -> Vec<(String, String, String)> {
    vec![
        (
            "Green Valley Farm".into(),
            "Organic raw dairy from pastured cows".into(),
            "2480".into(), // Lismore, NSW
        ),
        (
            "Mountain Creamery".into(),
            "Artisan cheese and butter".into(),
            "3741".into(), // Bright, VIC
        ),
        (
            "Sunrise Dairy".into(),
            "Fresh raw milk and kefir".into(),
            "4370".into(), // Warwick, QLD
        ),
        (
            "South Coast Organics".into(),
            "Certified organic dairy products".into(),
            "2546".into(), // Moruya, NSW
        ),
        (
            "Tasmania Pure".into(),
            "Heritage breed dairy, small batch".into(),
            "7250".into(), // Launceston, TAS
        ),
    ]
}
