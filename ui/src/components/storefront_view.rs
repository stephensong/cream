use dioxus::prelude::*;

use super::order_form::OrderForm;
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

#[component]
pub fn StorefrontView(supplier_name: String) -> Element {
    let user_state = use_user_state();
    let shared_state = use_shared_state();
    let mut selected_product = use_signal(|| None::<(String, u64)>);

    if let Some((product_name, price)) = selected_product.read().clone() {
        return rsx! {
            button {
                onclick: move |_| selected_product.set(None),
                "Back to Products"
            }
            OrderForm {
                supplier_name: supplier_name.clone(),
                product_name,
                price_per_unit: price,
            }
        };
    }

    // Check if this is the current user's storefront
    let is_own = user_state.read().moniker.as_ref() == Some(&supplier_name);

    // Always get products from SharedState (network-sourced storefronts)
    let products: Vec<(String, String, u64, u32)> = {
        let shared = shared_state.read();
        if let Some(storefront) = shared.storefronts.get(&supplier_name) {
            storefront
                .products
                .values()
                .map(|sp| {
                    let cat = format!("{:?}", sp.product.category);
                    (
                        sp.product.name.clone(),
                        cat,
                        sp.product.price_curd,
                        sp.product.quantity_available,
                    )
                })
                .collect()
        } else if cfg!(feature = "example-data") {
            example_products()
        } else {
            Vec::new()
        }
    };

    rsx! {
        div { class: "storefront-view",
            h2 { "{supplier_name}" }
            if is_own {
                p { class: "own-storefront-note",
                    "(This is your storefront — use the \"My Storefront\" tab to add products)"
                }
            }
            div { class: "product-list",
                if products.is_empty() {
                    p { class: "empty-state", "No products available." }
                } else {
                    {products.into_iter().map(|(name, category, price, qty)| {
                        let name_clone = name.clone();
                        let is_own_store = is_own;
                        rsx! {
                            div { class: "product-card",
                                key: "{name}",
                                h3 { "{name}" }
                                span { class: "category", "{category}" }
                                p { class: "price", "{price} CURD" }
                                p { class: "quantity", "Available: {qty}" }
                                if !is_own_store {
                                    button {
                                        onclick: move |_| selected_product.set(Some((name_clone.clone(), price))),
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

fn example_products() -> Vec<(String, String, u64, u32)> {
    vec![
        ("Raw Whole Milk (1 gal)".into(), "Milk".into(), 800, 25),
        ("Aged Cheddar (1 lb)".into(), "Cheese".into(), 1200, 15),
        ("Cultured Butter (8 oz)".into(), "Butter".into(), 600, 30),
        ("Fresh Cream (1 pt)".into(), "Cream".into(), 500, 20),
        ("Plain Kefir (1 qt)".into(), "Kefir".into(), 700, 12),
    ]
}
