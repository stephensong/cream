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

    // Build supplier list from the network directory
    let mut suppliers: Vec<SupplierEntry> = Vec::new();
    drop(state);

    // Add suppliers from the Freenet directory (SharedState),
    // filtering out the current user's own entry.
    let my_supplier_id = {
        let km_signal: Signal<Option<crate::components::key_manager::KeyManager>> = use_context();
        let km_guard = km_signal.read();
        km_guard.as_ref().map(|km| km.user_id())
    };
    {
        let shared = shared_state.read();
        for entry in shared.directory.entries.values() {
            // Skip our own entry — suppliers manage their storefront via "My Storefront"
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

    // Build market list
    let today = chrono::Utc::now().date_naive();
    let mut markets: Vec<(String, String, String, usize, Option<String>)> = Vec::new(); // (name, location, description, accepted_count, next_event)
    {
        let shared = shared_state.read();
        for market in shared.market_directory.entries.values() {
            let location = format_postcode(
                &market.postcode.clone().unwrap_or_default(),
                market.locality.as_deref(),
            );
            let accepted_count = market.accepted_suppliers().len();
            let next_event = market.next_event(today)
                .map(|e| format!("{} ({} – {})", e.date.format("%d %b"), e.start_time, e.end_time));
            markets.push((
                market.name.clone(),
                location,
                market.description.clone(),
                accepted_count,
                next_event,
            ));
        }
    }

    rsx! {
        div { class: "directory-view",
            // Markets section
            if !markets.is_empty() {
                h2 { "Farmer's Markets" }
                div { class: "market-list",
                    {markets.into_iter().map(|(name, location, desc, accepted_count, next_event)| {
                        let market_name = name.clone();
                        rsx! {
                            div { class: "market-card", key: "{name}",
                                h3 { "{name}" }
                                p { "{desc}" }
                                p { class: "location", "{location}" }
                                p { class: "supplier-count", "{accepted_count} suppliers" }
                                if let Some(ref evt) = next_event {
                                    p { class: "next-event", "Next: {evt}" }
                                }
                                Link {
                                    to: Route::Market { market_organizer: market_name },
                                    "View Market"
                                }
                            }
                        }
                    })}
                }
            }

            h2 { "Supplier Directory" }

            // Show connection status
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

