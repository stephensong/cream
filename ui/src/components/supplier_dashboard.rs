use dioxus::prelude::*;

#[component]
pub fn SupplierDashboard() -> Element {
    let mut show_add_product = use_signal(|| false);

    rsx! {
        div { class: "supplier-dashboard",
            h2 { "Supplier Dashboard" }

            div { class: "dashboard-section",
                h3 { "Your Products" }
                button {
                    onclick: move |_| show_add_product.set(!show_add_product()),
                    if *show_add_product.read() { "Cancel" } else { "Add Product" }
                }

                if *show_add_product.read() {
                    AddProductForm {}
                }

                if cfg!(feature = "example-data") {
                    div { class: "product-list",
                        p { "Raw Whole Milk - 25 available - 800 CURD" }
                        p { "Aged Cheddar - 15 available - 1200 CURD" }
                    }
                } else {
                    p { "No products yet. Add your first product above." }
                }
            }

            div { class: "dashboard-section",
                h3 { "Incoming Orders" }
                if cfg!(feature = "example-data") {
                    div { class: "order-list",
                        p { "Order #001 - Raw Milk x2 - Reserved (expires in 2 days)" }
                        p { "Order #002 - Cheddar x1 - Paid" }
                    }
                } else {
                    p { "No orders yet." }
                }
            }
        }
    }
}

#[component]
fn AddProductForm() -> Element {
    rsx! {
        div { class: "add-product-form",
            div { class: "form-group",
                label { "Product Name:" }
                input { r#type: "text", placeholder: "e.g., Raw Whole Milk (1 gal)" }
            }
            div { class: "form-group",
                label { "Category:" }
                select {
                    option { "Milk" }
                    option { "Cheese" }
                    option { "Butter" }
                    option { "Cream" }
                    option { "Yogurt" }
                    option { "Kefir" }
                    option { "Other" }
                }
            }
            div { class: "form-group",
                label { "Price (CURD):" }
                input { r#type: "number", min: "1", placeholder: "500" }
            }
            div { class: "form-group",
                label { "Quantity Available:" }
                input { r#type: "number", min: "0", placeholder: "10" }
            }
            div { class: "form-group",
                label { "Description:" }
                textarea { placeholder: "Describe your product..." }
            }
            button { "Save Product" }
        }
    }
}
