use dioxus::prelude::*;

use cream_common::currency::format_amount;
use cream_common::postcode::format_postcode;

use super::app::Route;
use super::shared_state::use_shared_state;

/// Market detail view — next event, accepted suppliers, aggregated products.
#[component]
pub fn MarketView(market_organizer: String) -> Element {
    let shared_state = use_shared_state();

    let shared = shared_state.read();

    // Find the market by name (case-insensitive)
    let market = shared.market_directory.entries.values()
        .find(|m| {
            m.name.to_lowercase() == market_organizer.to_lowercase()
        });

    let Some(market) = market else {
        return rsx! {
            div { class: "market-view",
                h2 { "Market Not Found" }
                p { "The market '{market_organizer}' could not be found." }
                Link { to: Route::Markets {}, "Back to Markets" }
            }
        };
    };

    let market_name = market.name.clone();
    let description = market.description.clone();
    let venue = market.venue_address.clone();
    let location_str = format_postcode(
        &market.postcode.clone().unwrap_or_default(),
        market.locality.as_deref(),
    );
    let timezone = market.timezone.clone();

    // Only show accepted suppliers
    let accepted_names: Vec<String> = market.accepted_suppliers()
        .into_iter().cloned().collect();

    // Next upcoming event
    let today = chrono::Utc::now().date_naive();
    let next_event = market.next_event(today);
    let next_event_str = next_event
        .map(|e| format!("{} ({} – {})", e.date.format("%a %d %b %Y"), e.start_time, e.end_time))
        .unwrap_or_else(|| "No upcoming events scheduled".to_string());

    // Future events (all on or after today)
    let future_events: Vec<cream_common::market::MarketEvent> = market.events.iter()
        .filter(|e| e.date >= today)
        .cloned()
        .collect();

    // Aggregate products from accepted suppliers, filtered by market_products selection
    struct MarketProduct {
        supplier_name: String,
        product_name: String,
        product_id: String,
        price_curd: u64,
        available: u32,
        category: String,
    }

    let mut products: Vec<MarketProduct> = Vec::new();
    for supplier_name in &accepted_names {
        if let Some(sf) = shared.storefronts.get(supplier_name) {
            // Check if supplier has a product selection for this market
            let selected = sf.info.market_products.get(&market_name);
            for sp in sf.products.values() {
                // Filter: if supplier has a selection and it's non-empty, only include those products
                if let Some(ids) = selected {
                    if !ids.is_empty() && !ids.contains(&sp.product.id) {
                        continue;
                    }
                }
                let available = sf.available_quantity(&sp.product.id);
                products.push(MarketProduct {
                    supplier_name: supplier_name.clone(),
                    product_name: sp.product.name.clone(),
                    product_id: sp.product.id.0.clone(),
                    price_curd: sp.product.price_curd,
                    available,
                    category: format!("{:?}", sp.product.category),
                });
            }
        }
    }
    drop(shared);

    rsx! {
        div { class: "market-view",
            h2 { "{market_name}" }
            p { class: "market-description", "{description}" }

            div { class: "market-info",
                h3 { "Venue" }
                p { "{venue}" }
                p { class: "location", "{location_str}" }

                if let Some(tz) = &timezone {
                    p { class: "timezone", "Timezone: {tz}" }
                }
            }

            div { class: "market-next-event",
                h3 { "Next Event" }
                p { class: "next-event-date", "{next_event_str}" }
            }

            if future_events.len() > 1 {
                div { class: "market-upcoming-events",
                    h3 { "Upcoming Events" }
                    for event in future_events.iter() {
                        {
                            let date_str = event.date.format("%a %d %b %Y").to_string();
                            let time_str = format!("{} – {}", event.start_time, event.end_time);
                            rsx! {
                                p { key: "{event.date}", "{date_str}  {time_str}" }
                            }
                        }
                    }
                }
            }

            div { class: "market-suppliers",
                h3 { "Participating Suppliers ({accepted_names.len()})" }
                div { class: "supplier-chips",
                    {accepted_names.iter().map(|name| {
                        rsx! {
                            Link {
                                key: "{name}",
                                to: Route::Supplier { name: name.clone() },
                                class: "supplier-chip",
                                "{name}"
                            }
                        }
                    })}
                }
            }

            div { class: "market-products",
                h3 { "Products Available ({products.len()})" }
                if products.is_empty() {
                    p { class: "empty-state", "No products currently listed by market suppliers." }
                } else {
                    div { class: "product-grid",
                        {products.into_iter().map(|p| {
                            let price_str = format_amount(p.price_curd);
                            let avail_class = if p.available == 0 { "out-of-stock" } else { "" };
                            rsx! {
                                div {
                                    class: "product-card {avail_class}",
                                    key: "{p.product_id}",
                                    h4 { "{p.product_name}" }
                                    p { class: "product-supplier", "From: {p.supplier_name}" }
                                    p { class: "product-category", "{p.category}" }
                                    p { class: "product-price", "{price_str}" }
                                    p { class: "product-availability",
                                        if p.available > 0 {
                                            "{p.available} available"
                                        } else {
                                            "Out of stock"
                                        }
                                    }
                                    if p.available > 0 {
                                        Link {
                                            to: Route::Supplier { name: p.supplier_name.clone() },
                                            class: "order-link",
                                            "Order from {p.supplier_name}"
                                        }
                                    }
                                }
                            }
                        })}
                    }
                }
            }

            Link { to: Route::Markets {}, class: "back-link", "Back to Markets" }
        }
    }
}
