use dioxus::prelude::*;

#[cfg(feature = "use-node")]
use super::node_api::{use_node_action, NodeAction};
use super::user_state::use_user_state;

#[component]
pub fn OrderForm(supplier_name: String, product_name: String, price_per_unit: u64) -> Element {
    let mut user_state = use_user_state();
    let mut quantity = use_signal(|| 1u32);
    let mut deposit_tier = use_signal(|| "2-Day Reserve (10%)".to_string());
    let mut submitted_id = use_signal(|| None::<u32>);

    if let Some(order_id) = *submitted_id.read() {
        return rsx! {
            div { class: "order-confirmation",
                h3 { "Order Submitted!" }
                p { "Order #{order_id}" }
                p { "Product: {product_name}" }
                p { "Quantity: {quantity}" }
                p { "Deposit tier: {deposit_tier}" }
                p { "Total: {price_per_unit * *quantity.read() as u64} CURD" }
                p { "Your reservation has been placed. View it in My Orders." }
            }
        };
    }

    let total = price_per_unit * *quantity.read() as u64;

    rsx! {
        div { class: "order-form",
            h2 { "Order: {product_name}" }
            p { "From: {supplier_name}" }
            p { "Price: {price_per_unit} CURD each" }
            div { class: "form-group",
                label { "Quantity:" }
                input {
                    r#type: "number",
                    min: "1",
                    value: "{quantity}",
                    oninput: move |evt| {
                        if let Ok(v) = evt.value().parse::<u32>() {
                            quantity.set(v);
                        }
                    },
                }
            }
            div { class: "form-group",
                label { "Deposit Tier:" }
                select {
                    value: "{deposit_tier}",
                    onchange: move |evt| deposit_tier.set(evt.value()),
                    option { value: "2-Day Reserve (10%)", "2-Day Reserve (10% deposit)" }
                    option { value: "1-Week Reserve (20%)", "1-Week Reserve (20% deposit)" }
                    option { value: "Full Payment (100%)", "Full Payment (100%)" }
                }
            }
            p { class: "order-total", "Total: {total} CURD" }
            button {
                onclick: {
                    let supplier = supplier_name.clone();
                    let product = product_name.clone();
                    move |_| {
                        let qty = *quantity.read();
                        let tier = deposit_tier.read().clone();

                        // Place order in local state
                        let id = user_state.write().place_order(
                            supplier.clone(),
                            product.clone(),
                            qty,
                            tier.clone(),
                            price_per_unit,
                        );

                        // Send to node if connected
                        #[cfg(feature = "use-node")]
                        {
                            let node = use_node_action();
                            node.send(NodeAction::PlaceOrder {
                                storefront_name: supplier.clone(),
                                product_id: product.clone(),
                                quantity: qty,
                                deposit_tier: tier,
                            });
                        }

                        submitted_id.set(Some(id));
                    }
                },
                "Place Order"
            }
        }
    }
}
