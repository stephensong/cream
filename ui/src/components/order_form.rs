use dioxus::prelude::*;

#[component]
pub fn OrderForm(product_name: String) -> Element {
    let mut quantity = use_signal(|| 1u32);
    let mut deposit_tier = use_signal(|| "reserve2days".to_string());
    let mut submitted = use_signal(|| false);

    if *submitted.read() {
        return rsx! {
            div { class: "order-confirmation",
                h3 { "Order Submitted!" }
                p { "Product: {product_name}" }
                p { "Quantity: {quantity}" }
                p { "Deposit tier: {deposit_tier}" }
                p { "Your reservation has been placed." }
            }
        };
    }

    rsx! {
        div { class: "order-form",
            h2 { "Order: {product_name}" }
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
                    option { value: "reserve2days", "2-Day Reserve (10% deposit)" }
                    option { value: "reserve1week", "1-Week Reserve (20% deposit)" }
                    option { value: "fullpayment", "Full Payment (100%)" }
                }
            }
            button {
                onclick: move |_| submitted.set(true),
                "Place Order"
            }
        }
    }
}
