use dioxus::prelude::*;

use cream_common::currency::format_amount;
use cream_common::message::Message;
use cream_common::storefront::WeeklySchedule;

use super::node_api::{use_node_action, NodeAction};
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

    // Check if this is the current user's storefront and if user is registered
    let state = user_state.read();
    let is_own = state.moniker.as_ref() == Some(&supplier_name);
    let is_registered = state.user_contract_key.is_some();
    drop(state);

    // Get schedule + timezone + contact details for the storefront header
    let (storefront_schedule, storefront_timezone, contact_phone, contact_email, contact_address): (
        Option<WeeklySchedule>, Option<String>, Option<String>, Option<String>, Option<String>,
    ) = {
        let shared = shared_state.read();
        shared
            .storefronts
            .get(&supplier_name)
            .map(|sf| (
                sf.info.schedule.clone(),
                sf.info.timezone.clone(),
                sf.info.phone.clone(),
                sf.info.email.clone(),
                sf.info.address.clone(),
            ))
            .unwrap_or((None, None, None, None, None))
    };
    let has_contact = contact_phone.is_some() || contact_email.is_some() || contact_address.is_some();

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
            if has_contact {
                div { class: "contact-details",
                    h3 { "Contact Details" }
                    if let Some(ref phone) = contact_phone {
                        p {
                            span { class: "contact-label", "Phone: " }
                            a { href: "tel:{phone}", "{phone}" }
                        }
                    }
                    if let Some(ref email) = contact_email {
                        p {
                            span { class: "contact-label", "Email: " }
                            span { "{email}" }
                        }
                    }
                    if let Some(ref address) = contact_address {
                        p {
                            span { class: "contact-label", "Address: " }
                            span { "{address}" }
                        }
                    }
                }
            }
            if is_own {
                p { class: "own-storefront-note",
                    "(This is your storefront — use the \"My Storefront\" tab to add products)"
                }
            }
            if is_registered && !is_own {
                MessageSection { supplier_name: supplier_name.clone() }
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
                                if !is_own_store && is_registered {
                                    button {
                                        onclick: move |_| selected_product.set(Some((pid.clone(), name_clone.clone(), price))),
                                        "Order"
                                    }
                                }
                                if !is_own_store && !is_registered {
                                    p { class: "guest-hint", "Register to place orders" }
                                }
                            }
                        }
                    })}
                }
            }
        }
    }
}

/// Messaging section: compose + thread view for a supplier's storefront.
#[component]
fn MessageSection(supplier_name: String) -> Element {
    let shared_state = use_shared_state();
    let user_state = use_user_state();
    let mut msg_body = use_signal(String::new);
    let mut send_error = use_signal(|| None::<String>);

    let balance = user_state.read().balance();
    let my_name = user_state.read().moniker.clone().unwrap_or_default();

    // Get messages for this storefront
    let messages: Vec<Message> = {
        let shared = shared_state.read();
        shared
            .storefronts
            .get(&supplier_name)
            .map(|sf| {
                let mut msgs: Vec<_> = sf.messages.values().cloned().collect();
                msgs.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                msgs
            })
            .unwrap_or_default()
    };

    rsx! {
        div { class: "message-section",
            h3 { "Messages" }
            if !messages.is_empty() {
                div { class: "message-thread",
                    {messages.iter().map(|msg| {
                        let is_mine = msg.sender_name == my_name;
                        let bubble_class = if is_mine { "message-bubble message-sent" } else { "message-bubble message-received" };
                        let time_str = msg.created_at.format("%d %b %H:%M").to_string();
                        let indent = msg.reply_to.is_some();
                        rsx! {
                            div {
                                class: if indent { format!("{bubble_class} message-reply") } else { bubble_class.to_string() },
                                key: "{msg.id}",
                                div { class: "message-header",
                                    span { class: "message-sender", "{msg.sender_name}" }
                                    span { class: "message-time", "{time_str}" }
                                }
                                p { class: "message-body", "{msg.body}" }
                            }
                        }
                    })}
                }
            }
            div { class: "message-input",
                textarea {
                    placeholder: "Write a message...",
                    maxlength: "1000",
                    value: "{msg_body}",
                    oninput: move |evt| {
                        msg_body.set(evt.value());
                        send_error.set(None);
                    },
                }
                div { class: "message-input-footer",
                    span { class: "toll-badge", "Cost: 10 CURD" }
                    if let Some(err) = send_error.read().as_ref() {
                        span { class: "field-error", "{err}" }
                    }
                    button {
                        disabled: msg_body.read().trim().is_empty() || balance < 10,
                        onclick: {
                            let supplier_name = supplier_name.clone();
                            move |_| {
                                let body = msg_body.read().trim().to_string();
                                if body.is_empty() {
                                    return;
                                }
                                if balance < 10 {
                                    send_error.set(Some("Insufficient balance (need 10 CURD)".into()));
                                    return;
                                }
                                let node = use_node_action();
                                node.send(NodeAction::SendMessage {
                                    supplier_name: supplier_name.clone(),
                                    body,
                                    reply_to: None,
                                });
                                msg_body.set(String::new());
                            }
                        },
                        "Send (10 CURD)"
                    }
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

