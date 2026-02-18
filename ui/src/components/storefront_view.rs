use dioxus::prelude::*;

use super::order_form::OrderForm;

#[component]
pub fn StorefrontView(supplier_name: String) -> Element {
    let mut selected_product = use_signal(|| None::<String>);

    if let Some(product_name) = selected_product.read().clone() {
        return rsx! {
            button {
                onclick: move |_| selected_product.set(None),
                "Back to Products"
            }
            OrderForm { product_name }
        };
    }

    rsx! {
        div { class: "storefront-view",
            h2 { "{supplier_name}" }
            div { class: "product-list",
                if cfg!(feature = "example-data") {
                    {example_products().into_iter().map(|(name, category, price, qty)| {
                        let name_clone = name.clone();
                        rsx! {
                            div { class: "product-card",
                                key: "{name}",
                                h3 { "{name}" }
                                span { class: "category", "{category}" }
                                p { class: "price", "{price} CURD" }
                                p { class: "quantity", "Available: {qty}" }
                                button {
                                    onclick: move |_| selected_product.set(Some(name_clone.clone())),
                                    "Order"
                                }
                            }
                        }
                    })}
                } else {
                    p { "Connect to Freenet to view products." }
                }
            }
        }
    }
}

fn example_products() -> Vec<(String, String, u64, u32)> {
    vec![
        ("Raw Whole Milk (1 gal)".into(), "Milk".into(), 800, 25),
        ("Aged Cheddar (1 lb)".into(), "Cheese".into(), 1200, 15),
        ("Cultured Butter (8 oz)".into(), "Butter".into(), 600, 30),
        ("Fresh Cream (1 pt)".into(), "Cream".into(), 500, 20),
        ("Plain Kefir (1 qt)".into(), "Kefir".into(), 700, 12),
    ]
}
