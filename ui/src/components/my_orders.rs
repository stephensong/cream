use dioxus::prelude::*;

use cream_common::currency::format_amount;

use super::user_state::use_user_state;

#[component]
pub fn MyOrders() -> Element {
    let user_state = use_user_state();
    let state = user_state.read();
    let currency = state.currency.clone();
    let orders = &state.orders;

    rsx! {
        div { class: "my-orders",
            h2 { "My Orders" }
            if orders.is_empty() {
                p { class: "empty-state", "You haven't placed any orders yet. Browse suppliers to get started!" }
            } else {
                div { class: "order-list",
                    {orders.iter().map(|order| {
                        let total = order.price_per_unit * order.quantity as u64;
                        let total_str = format_amount(total, &currency);
                        rsx! {
                            div { class: "order-card",
                                key: "{order.id}",
                                div { class: "order-header",
                                    span { class: "order-id", "Order #{order.id}" }
                                    span { class: "order-status", " — {order.status}" }
                                }
                                p { class: "order-product", "{order.product}" }
                                p { class: "order-supplier", "From: {order.supplier}" }
                                p { "Qty: {order.quantity} | Deposit: {order.deposit_tier}" }
                                p { class: "order-total", "Total: {total_str}" }
                            }
                        }
                    })}
                }
            }
        }
    }
}
