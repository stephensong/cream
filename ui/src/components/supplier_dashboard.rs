use dioxus::prelude::*;

use cream_common::postcode::format_postcode;

#[cfg(feature = "use-node")]
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

#[component]
pub fn SupplierDashboard() -> Element {
    let user_state = use_user_state();
    let shared_state = use_shared_state();
    let mut show_add_product = use_signal(|| false);

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
    let network_orders: Vec<_> = storefront
        .map(|sf| sf.orders.values().cloned().collect())
        .unwrap_or_default();
    drop(shared);

    rsx! {
        div { class: "supplier-dashboard",
            h2 { "My Storefront" }

            div { class: "dashboard-section",
                h3 { "Storefront Info" }
                p { "Name: {moniker}" }
                p { "Location: {postcode}" }
                p { "Description: {description}" }
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
                            rsx! {
                                div { class: "product-card",
                                    key: "{pid}",
                                    div { class: "product-header",
                                        h4 { "{product.name}" }
                                        span { class: "category", "{product.category:?}" }
                                    }
                                    p { "{product.description}" }
                                    p { "Price: {product.price_curd} CURD | Available: {available}" }
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
                            let pid = order.product_id.0.clone();
                            let status = format!("{:?}", order.status);
                            let tier = format!("{:?}", order.deposit_tier);
                            rsx! {
                                div { class: "order-card",
                                    key: "{oid}",
                                    span { class: "order-id", "Order #{oid}" }
                                    span { class: "order-status", " - {status}" }
                                    p { "{pid} x{order.quantity} - {order.total_price} CURD" }
                                    p { "Deposit: {tier} ({order.deposit_amount} CURD)" }
                                }
                            }
                        })}
                    }
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

                    // Send to node if connected
                    #[cfg(feature = "use-node")]
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
