use dioxus::prelude::*;

use cream_common::currency::format_amount;
use cream_common::storefront::WeeklySchedule;

use super::order_form::OrderForm;
use super::schedule_editor::ScheduleSummary;
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

#[component]
pub fn StorefrontView(supplier_name: String) -> Element {
    let user_state = use_user_state();
    let shared_state = use_shared_state();
    let mut selected_product = use_signal(|| None::<(String, String, u64)>);

    if let Some((product_id, product_name, price)) = selected_product.read().clone() {
        return rsx! {
            button {
                onclick: move |_| selected_product.set(None),
                "Back to Products"
            }
            OrderForm {
                supplier_name: supplier_name.clone(),
                product_id,
                product_name,
                price_per_unit: price,
            }
        };
    }

    // Check if this is the current user's storefront
    let state = user_state.read();
    let is_own = state.moniker.as_ref() == Some(&supplier_name);
    drop(state);

    // Get schedule + timezone for the open/closed badge
    let (storefront_schedule, storefront_timezone): (Option<WeeklySchedule>, Option<String>) = {
        let shared = shared_state.read();
        shared
            .storefronts
            .get(&supplier_name)
            .map(|sf| (sf.info.schedule.clone(), sf.info.timezone.clone()))
            .unwrap_or((None, None))
    };

    // Always get products from SharedState (network-sourced storefronts)
    // Tuple: (product_id, name, category, price, available_quantity)
    let products: Vec<(String, String, String, u64, u32)> = {
        let shared = shared_state.read();
        if let Some(storefront) = shared.storefronts.get(&supplier_name) {
            storefront
                .products
                .values()
                .map(|sp| {
                    let cat = format!("{:?}", sp.product.category);
                    let available = storefront.available_quantity(&sp.product.id);
                    (
                        sp.product.id.0.clone(),
                        sp.product.name.clone(),
                        cat,
                        sp.product.price_curd,
                        available,
                    )
                })
                .collect()
        } else {
            Vec::new()
        }
    };

    rsx! {
        div { class: "storefront-view",
            div { class: "storefront-heading",
                h2 { "{supplier_name}" }
                if let Some(ref schedule) = storefront_schedule {
                    OpenClosedBadge { schedule: schedule.clone(), timezone: storefront_timezone.clone() }
                }
            }
            if let Some(ref schedule) = storefront_schedule {
                ScheduleSummary { schedule: schedule.clone() }
            }
            if is_own {
                p { class: "own-storefront-note",
                    "(This is your storefront — use the \"My Storefront\" tab to add products)"
                }
            }
            div { class: "product-list",
                if products.is_empty() {
                    p { class: "empty-state", "No products available." }
                } else {
                    {products.into_iter().map(|(product_id, name, category, price, qty)| {
                        let pid = product_id.clone();
                        let name_clone = name.clone();
                        let is_own_store = is_own;
                        let price_str = format_amount(price);
                        rsx! {
                            div { class: "product-card",
                                key: "{product_id}",
                                h3 { "{name}" }
                                span { class: "category", "{category}" }
                                p { class: "price", "{price_str}" }
                                p { class: "quantity", "Available: {qty}" }
                                if !is_own_store {
                                    button {
                                        onclick: move |_| selected_product.set(Some((pid.clone(), name_clone.clone(), price))),
                                        "Order"
                                    }
                                }
                            }
                        }
                    })}
                }
            }
        }
    }
}

/// Badge showing "Open" (green) or "Closed" (red) based on the current time.
#[component]
fn OpenClosedBadge(schedule: WeeklySchedule, timezone: Option<String>) -> Element {
    let is_open = use_memo(move || {
        let offset = timezone
            .as_deref()
            .and_then(get_utc_offset_minutes)
            .unwrap_or(0);
        schedule.is_currently_open(offset)
    });

    let (class, label) = if is_open() {
        ("badge badge-open", "Open")
    } else {
        ("badge badge-closed", "Closed")
    };

    rsx! {
        span { class: "{class}", "{label}" }
    }
}

/// Get the current UTC offset in minutes for an IANA timezone name.
/// Uses JavaScript's Intl API in WASM builds; returns None on failure.
fn get_utc_offset_minutes(tz: &str) -> Option<i32> {
    #[cfg(target_family = "wasm")]
    {
        // Use JS to get offset: new Date().toLocaleString("en-US", {timeZone}) then compare
        // Simpler: use Intl.DateTimeFormat resolvedOptions + getTimezoneOffset trick
        let js_code = format!(
            r#"(function() {{
                try {{
                    var now = new Date();
                    var utcStr = now.toLocaleString("en-US", {{timeZone: "UTC"}});
                    var tzStr = now.toLocaleString("en-US", {{timeZone: "{}"}});
                    var utcDate = new Date(utcStr);
                    var tzDate = new Date(tzStr);
                    return Math.round((tzDate - utcDate) / 60000);
                }} catch(e) {{
                    return null;
                }}
            }})()"#,
            tz
        );
        let result = web_sys::js_sys::eval(&js_code).ok()?;
        result.as_f64().map(|f| f as i32)
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = tz;
        None
    }
}

