use dioxus::prelude::*;

use cream_common::currency::format_amount;
use cream_common::message::Message;
use cream_common::postcode::format_postcode;
use cream_common::storefront::WeeklySchedule;

use super::schedule_editor::{ScheduleEditor, ScheduleSummary};
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

#[component]
pub fn SupplierDashboard() -> Element {
    let user_state = use_user_state();
    let mut shared_state = use_shared_state();
    let mut show_add_product = use_signal(|| false);
    let mut editing_schedule = use_signal(|| false);
    let mut schedule_edit_gen = use_signal(|| 0u32);
    let mut editing_product = use_signal(|| None::<String>);
    let mut edit_price = use_signal(String::new);
    let mut edit_quantity = use_signal(String::new);
    let mut editing_contact = use_signal(|| false);
    let mut contact_phone = use_signal(String::new);
    let mut contact_email = use_signal(String::new);
    let mut contact_address = use_signal(String::new);

    let state = user_state.read();
    let moniker = state.moniker.clone().unwrap_or_default();
    let postcode = format_postcode(
        &state.postcode.clone().unwrap_or("?".into()),
        state.locality.as_deref(),
    );
    let description = state
        .supplier_description
        .clone()
        .unwrap_or("No description set".into());
    drop(state);

    // Get products (with computed available quantity) and orders from the network storefront
    let shared = shared_state.read();
    let storefront = shared.storefronts.get(&moniker);
    // Tuple: (product, available_quantity)
    let products: Vec<(cream_common::product::Product, u32)> = storefront
        .map(|sf| {
            sf.products
                .values()
                .map(|sp| {
                    let available = sf.available_quantity(&sp.product.id);
                    (sp.product.clone(), available)
                })
                .collect()
        })
        .unwrap_or_default();
    let current_schedule: WeeklySchedule = storefront
        .and_then(|sf| sf.info.schedule.clone())
        .unwrap_or_default();
    let current_phone: Option<String> = storefront.and_then(|sf| sf.info.phone.clone());
    let current_email: Option<String> = storefront.and_then(|sf| sf.info.email.clone());
    let current_address: Option<String> = storefront.and_then(|sf| sf.info.address.clone());
    let network_orders: Vec<_> = storefront
        .map(|sf| sf.orders.values().cloned().collect())
        .unwrap_or_default();
    let network_messages: Vec<Message> = storefront
        .map(|sf| {
            let mut msgs: Vec<_> = sf.messages.values().cloned().collect();
            msgs.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // newest first
            msgs
        })
        .unwrap_or_default();
    // Map product IDs to names for readable order display
    let product_names: std::collections::HashMap<String, String> = storefront
        .map(|sf| {
            sf.products
                .iter()
                .map(|(id, sp)| (id.0.clone(), sp.product.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    drop(shared);

    let moniker_for_contact = moniker.clone();
    let moniker_for_messages = moniker.clone();

    rsx! {
        div { class: "supplier-dashboard",
            h2 { "My Storefront" }

            div { class: "dashboard-section",
                h3 { "Storefront Info" }
                p { "Name: {moniker}" }
                p { "Location: {postcode}" }
                p { "Description: {description}" }
                ShareableUrl { moniker: moniker.clone() }
            }

            div { class: "dashboard-section",
                h3 { "Opening Hours" }
                if *editing_schedule.read() {
                    ScheduleEditor {
                        key: "{schedule_edit_gen}",
                        schedule: current_schedule.clone(),
                        on_save: move |sched: WeeklySchedule| {
                            // Update local state immediately so re-renders see
                            // the saved schedule without waiting for the coroutine.
                            {
                                let tz = user_state.read().postcode.as_deref()
                                    .and_then(cream_common::postcode::timezone_for_postcode)
                                    .map(|s: &str| s.to_string());
                                let mut shared = shared_state.write();
                                if let Some(sf) = shared.storefronts.get_mut(&moniker) {
                                    sf.info.schedule = Some(sched.clone());
                                    sf.info.timezone = tz;
                                }
                            }

                            {
                                let node = use_node_action();
                                node.send(NodeAction::UpdateSchedule { schedule: sched });
                            }
                            editing_schedule.set(false);
                        },
                        on_cancel: move |_| {
                            editing_schedule.set(false);
                        },
                    }
                } else {
                    ScheduleSummary { schedule: current_schedule.clone() }
                    button {
                        onclick: move |_| {
                            schedule_edit_gen += 1;
                            editing_schedule.set(true);
                        },
                        "Edit Hours"
                    }
                }
            }

            div { class: "dashboard-section",
                h3 { "Contact Details" }
                if *editing_contact.read() {
                    div { class: "privacy-warning",
                        "Contact details are publicly visible to all customers. Only share what you're comfortable with."
                    }
                    div { class: "form-group",
                        label { "Phone:" }
                        input {
                            r#type: "tel",
                            placeholder: "e.g., 0412 345 678",
                            value: "{contact_phone}",
                            oninput: move |evt| contact_phone.set(evt.value()),
                        }
                    }
                    div { class: "form-group",
                        label { "Email:" }
                        input {
                            r#type: "email",
                            placeholder: "e.g., farm@example.com",
                            value: "{contact_email}",
                            oninput: move |evt| contact_email.set(evt.value()),
                        }
                    }
                    div { class: "form-group",
                        label { "Address:" }
                        input {
                            r#type: "text",
                            placeholder: "e.g., 42 Dairy Lane, Cowville NSW 2000",
                            value: "{contact_address}",
                            oninput: move |evt| contact_address.set(evt.value()),
                        }
                    }
                    div { class: "schedule-actions",
                        button {
                            onclick: {
                                let moniker = moniker_for_contact.clone();
                                move |_| {
                                let phone = {
                                    let v = contact_phone.read().trim().to_string();
                                    if v.is_empty() { None } else { Some(v) }
                                };
                                let email = {
                                    let v = contact_email.read().trim().to_string();
                                    if v.is_empty() { None } else { Some(v) }
                                };
                                let address = {
                                    let v = contact_address.read().trim().to_string();
                                    if v.is_empty() { None } else { Some(v) }
                                };

                                // Optimistic update
                                {
                                    let mut shared = shared_state.write();
                                    if let Some(sf) = shared.storefronts.get_mut(&moniker) {
                                        sf.info.phone = phone.clone();
                                        sf.info.email = email.clone();
                                        sf.info.address = address.clone();
                                    }
                                }

                                {
                                    let node = use_node_action();
                                    node.send(NodeAction::UpdateContactDetails { phone, email, address });
                                }
                                editing_contact.set(false);
                            }},
                            "Save Contact Details"
                        }
                        button {
                            onclick: move |_| editing_contact.set(false),
                            "Cancel"
                        }
                    }
                } else {
                    if current_phone.is_some() || current_email.is_some() || current_address.is_some() {
                        div { class: "contact-details",
                            if let Some(ref phone) = current_phone {
                                p { "Phone: {phone}" }
                            }
                            if let Some(ref email) = current_email {
                                p { "Email: {email}" }
                            }
                            if let Some(ref address) = current_address {
                                p { "Address: {address}" }
                            }
                        }
                    } else {
                        p { class: "empty-state", "No contact details set." }
                    }
                    button {
                        onclick: {
                            let cp = current_phone.clone();
                            let ce = current_email.clone();
                            let ca = current_address.clone();
                            move |_| {
                                contact_phone.set(cp.clone().unwrap_or_default());
                                contact_email.set(ce.clone().unwrap_or_default());
                                contact_address.set(ca.clone().unwrap_or_default());
                                editing_contact.set(true);
                            }
                        },
                        "Edit Contact Details"
                    }
                }
            }

            div { class: "dashboard-section",
                h3 { "Your Products ({products.len()})" }
                button {
                    onclick: move |_| show_add_product.set(!show_add_product()),
                    if *show_add_product.read() { "Cancel" } else { "Add Product" }
                }

                if *show_add_product.read() {
                    AddProductForm { on_added: move || show_add_product.set(false) }
                }

                if products.is_empty() {
                    p { class: "empty-state", "No products yet. Add your first product above." }
                } else {
                    div { class: "product-list",
                        {products.iter().map(|(product, available)| {
                            let pid = product.id.0.clone();
                            let price_str = format_amount(product.price_curd);
                            let is_editing = editing_product.read().as_deref() == Some(&pid);
                            let pid_edit = pid.clone();
                            let pid_save = pid.clone();
                            let current_price = product.price_curd;
                            let current_qty = product.quantity_total;
                            rsx! {
                                div { class: "product-card",
                                    key: "{pid}",
                                    div { class: "product-header",
                                        h4 { "{product.name}" }
                                        span { class: "category", "{product.category:?}" }
                                    }
                                    p { "{product.description}" }
                                    if is_editing {
                                        div { class: "edit-product-form",
                                            div { class: "form-group",
                                                label { "Price (CURD):" }
                                                input {
                                                    r#type: "number",
                                                    min: "1",
                                                    value: "{edit_price}",
                                                    oninput: move |evt| edit_price.set(evt.value()),
                                                }
                                            }
                                            div { class: "form-group",
                                                label { "Quantity:" }
                                                input {
                                                    r#type: "number",
                                                    min: "0",
                                                    value: "{edit_quantity}",
                                                    oninput: move |evt| edit_quantity.set(evt.value()),
                                                }
                                            }
                                            button {
                                                onclick: move |_| {
                                                    let p = edit_price.read().trim().parse::<u64>().unwrap_or(0);
                                                    let q = edit_quantity.read().trim().parse::<u32>().unwrap_or(0);
                                                    if p > 0 {
                                                        let node = use_node_action();
                                                        node.send(NodeAction::UpdateProduct {
                                                            product_id: pid_save.clone(),
                                                            price_curd: p,
                                                            quantity_total: q,
                                                        });
                                                    }
                                                    editing_product.set(None);
                                                },
                                                "Save"
                                            }
                                            button {
                                                onclick: move |_| {
                                                    editing_product.set(None);
                                                },
                                                "Cancel"
                                            }
                                        }
                                    } else {
                                        p { class: "quantity", "Price: {price_str} | Available: {available}" }
                                        button {
                                            onclick: move |_| {
                                                editing_product.set(Some(pid_edit.clone()));
                                                edit_price.set(current_price.to_string());
                                                edit_quantity.set(current_qty.to_string());
                                            },
                                            "Edit"
                                        }
                                    }
                                }
                            }
                        })}
                    }
                }
            }

            div { class: "dashboard-section",
                h3 { "Incoming Orders ({network_orders.len()})" }
                if network_orders.is_empty() {
                    p { class: "empty-state", "No orders yet." }
                } else {
                    div { class: "order-list",
                        {network_orders.iter().map(|order| {
                            let oid = order.id.0.clone();
                            let short_id = if oid.len() > 4 { &oid[oid.len()-4..] } else { &oid };
                            let product_name = product_names
                                .get(&order.product_id.0)
                                .cloned()
                                .unwrap_or_else(|| order.product_id.0.clone());
                            let status = order.status.to_string();
                            let deposit_str = format_amount(order.deposit_amount);
                            let total_str = format_amount(order.total_price);
                            let deposit_info = match &order.status {
                                cream_common::order::OrderStatus::Reserved { expires_at } => {
                                    let pct = (order.deposit_tier.deposit_fraction() * 100.0) as u32;
                                    format!("Held until {} ({pct}% deposit: {deposit_str})", expires_at.format("%d %b %Y"))
                                }
                                _ => {
                                    format!("Deposit: {} ({deposit_str})", order.deposit_tier)
                                }
                            };
                            let can_cancel = matches!(
                                order.status,
                                cream_common::order::OrderStatus::Reserved { .. }
                                    | cream_common::order::OrderStatus::Paid
                            );
                            let cancel_oid = oid.clone();
                            rsx! {
                                div { class: "order-card",
                                    key: "{oid}",
                                    span { class: "order-id", "Order #{short_id}" }
                                    span { class: "order-status", " — {status}" }
                                    p { "{product_name} x{order.quantity} — {total_str}" }
                                    p { "{deposit_info}" }
                                    if can_cancel {
                                        button {
                                            class: "cancel-order-btn",
                                            onclick: move |_| {
                                                let node = use_node_action();
                                                node.send(NodeAction::CancelOrder {
                                                    order_id: cancel_oid.clone(),
                                                });
                                            },
                                            "Cancel Order"
                                        }
                                    }
                                }
                            }
                        })}
                    }
                }
            }

            SupplierMessages { messages: network_messages, supplier_name: moniker_for_messages }
        }
    }
}

/// Messages section in the supplier dashboard with reply capability.
#[component]
fn SupplierMessages(messages: Vec<Message>, supplier_name: String) -> Element {
    let mut reply_to = use_signal(|| None::<u64>);
    let mut reply_body = use_signal(String::new);
    let user_state = use_user_state();
    let balance = user_state.read().balance();

    rsx! {
        div { class: "dashboard-section",
            h3 { "Messages ({messages.len()})" }
            if messages.is_empty() {
                p { class: "empty-state", "No messages yet." }
            } else {
                div { class: "message-thread",
                    {messages.iter().map(|msg| {
                        let msg_id = msg.id;
                        let is_own = msg.sender_name == supplier_name;
                        let bubble_class = if is_own { "message-bubble message-sent" } else { "message-bubble message-received" };
                        let time_str = msg.created_at.format("%d %b %H:%M").to_string();
                        let indent = msg.reply_to.is_some();
                        let is_replying = *reply_to.read() == Some(msg_id);
                        let supplier_name = supplier_name.clone();
                        rsx! {
                            div {
                                class: if indent { format!("{bubble_class} message-reply") } else { bubble_class.to_string() },
                                key: "{msg_id}",
                                div { class: "message-header",
                                    span { class: "message-sender", "{msg.sender_name}" }
                                    span { class: "message-time", "{time_str}" }
                                }
                                p { class: "message-body", "{msg.body}" }
                                if !is_own && !is_replying {
                                    button {
                                        class: "schedule-add-btn",
                                        onclick: move |_| {
                                            reply_to.set(Some(msg_id));
                                            reply_body.set(String::new());
                                        },
                                        "Reply"
                                    }
                                }
                                if is_replying {
                                    div { class: "message-input",
                                        textarea {
                                            placeholder: "Write a reply...",
                                            maxlength: "1000",
                                            value: "{reply_body}",
                                            oninput: move |evt| reply_body.set(evt.value()),
                                        }
                                        div { class: "message-input-footer",
                                            span { class: "toll-badge", "Cost: 10 CURD" }
                                            button {
                                                disabled: reply_body.read().trim().is_empty() || balance < 10,
                                                onclick: {
                                                    let supplier_name = supplier_name.clone();
                                                    move |_| {
                                                        let body = reply_body.read().trim().to_string();
                                                        if body.is_empty() { return; }
                                                        let node = use_node_action();
                                                        node.send(NodeAction::SendMessage {
                                                            supplier_name: supplier_name.clone(),
                                                            body,
                                                            reply_to: Some(msg_id),
                                                        });
                                                        reply_to.set(None);
                                                        reply_body.set(String::new());
                                                    }
                                                },
                                                "Send Reply (10 CURD)"
                                            }
                                            button {
                                                onclick: move |_| reply_to.set(None),
                                                "Cancel"
                                            }
                                        }
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

#[component]
fn AddProductForm(on_added: EventHandler<()>) -> Element {
    let mut user_state = use_user_state();
    let mut name = use_signal(String::new);
    let mut category = use_signal(|| "Milk".to_string());
    let mut description = use_signal(String::new);
    let mut price = use_signal(String::new);
    let mut quantity = use_signal(String::new);

    let can_submit = use_memo(move || {
        let name_ok = !name.read().trim().is_empty();
        let price_ok = price.read().trim().parse::<u64>().is_ok();
        let qty_ok = quantity.read().trim().parse::<u32>().is_ok();
        name_ok && price_ok && qty_ok
    });

    rsx! {
        div { class: "add-product-form",
            div { class: "form-group",
                label { "Product Name:" }
                input {
                    r#type: "text",
                    placeholder: "e.g., Raw Whole Milk (1 gal)",
                    value: "{name}",
                    oninput: move |evt| name.set(evt.value()),
                }
            }
            div { class: "form-group",
                label { "Category:" }
                select {
                    value: "{category}",
                    onchange: move |evt| category.set(evt.value()),
                    option { value: "Milk", "Milk" }
                    option { value: "Cheese", "Cheese" }
                    option { value: "Butter", "Butter" }
                    option { value: "Cream", "Cream" }
                    option { value: "Yogurt", "Yogurt" }
                    option { value: "Kefir", "Kefir" }
                    option { value: "Other", "Other" }
                }
            }
            div { class: "form-group",
                label { "Price (CURD):" }
                input {
                    r#type: "number",
                    min: "1",
                    placeholder: "500",
                    value: "{price}",
                    oninput: move |evt| price.set(evt.value()),
                }
            }
            div { class: "form-group",
                label { "Quantity Available:" }
                input {
                    r#type: "number",
                    min: "0",
                    placeholder: "10",
                    value: "{quantity}",
                    oninput: move |evt| quantity.set(evt.value()),
                }
            }
            div { class: "form-group",
                label { "Description:" }
                textarea {
                    placeholder: "Describe your product...",
                    value: "{description}",
                    oninput: move |evt| description.set(evt.value()),
                }
            }
            button {
                disabled: !can_submit(),
                onclick: move |_| {
                    let p = price.read().trim().parse::<u64>().unwrap_or(0);
                    let q = quantity.read().trim().parse::<u32>().unwrap_or(0);
                    let prod_name = name.read().trim().to_string();
                    let prod_cat = category.read().clone();
                    let prod_desc = description.read().trim().to_string();

                    // Add to local state
                    user_state.write().add_product(
                        prod_name.clone(),
                        prod_cat.clone(),
                        prod_desc.clone(),
                        p,
                        q,
                    );

                    // Send to node
                    {
                        let node = use_node_action();
                        node.send(NodeAction::AddProduct {
                            name: prod_name,
                            category: prod_cat,
                            description: prod_desc,
                            price_curd: p,
                            quantity_total: q,
                        });
                    }

                    on_added.call(());
                },
                "Save Product"
            }
        }
    }
}

#[component]
fn ShareableUrl(moniker: String) -> Element {
    #[cfg(target_family = "wasm")]
    {
        let shareable_url = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .map(|origin| format!("{}/?supplier={}", origin, moniker.to_lowercase()));

        let mut copy_status = use_signal(|| None::<&'static str>);

        if let Some(url) = shareable_url {
            return rsx! {
                div { class: "shareable-url",
                    label { "Share with customers:" }
                    div { class: "url-copy-row",
                        code { "{url}" }
                        button {
                            onclick: {
                                let url = url.clone();
                                move |_| {
                                    let url = url.clone();
                                    spawn(async move {
                                        if let Some(window) = web_sys::window() {
                                            let clipboard = window.navigator().clipboard();
                                            let promise = clipboard.write_text(&url);
                                            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                                            copy_status.set(Some("Copied!"));
                                        }
                                    });
                                }
                            },
                            if let Some(status) = *copy_status.read() {
                                "{status}"
                            } else {
                                "Copy"
                            }
                        }
                    }
                }
            };
        }
    }

    rsx! {}
}
