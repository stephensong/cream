use dioxus::prelude::*;

use super::storefront_view::StorefrontView;

#[component]
pub fn DirectoryView() -> Element {
    let mut selected_supplier = use_signal(|| None::<String>);
    let mut search_query = use_signal(|| String::new());

    if let Some(supplier_name) = selected_supplier.read().clone() {
        return rsx! {
            button {
                onclick: move |_| selected_supplier.set(None),
                "Back to Directory"
            }
            StorefrontView { supplier_name }
        };
    }

    rsx! {
        div { class: "directory-view",
            h2 { "Supplier Directory" }
            div { class: "search-bar",
                input {
                    r#type: "text",
                    placeholder: "Search suppliers...",
                    value: "{search_query}",
                    oninput: move |evt| search_query.set(evt.value()),
                }
            }
            div { class: "supplier-list",
                if cfg!(feature = "example-data") {
                    // Example suppliers for development
                    {example_suppliers().into_iter().map(|(name, desc, location)| {
                        let name_clone = name.clone();
                        rsx! {
                            div { class: "supplier-card",
                                key: "{name}",
                                h3 { "{name}" }
                                p { "{desc}" }
                                p { class: "location", "{location}" }
                                button {
                                    onclick: move |_| selected_supplier.set(Some(name_clone.clone())),
                                    "View Storefront"
                                }
                            }
                        }
                    })}
                } else {
                    p { "Connect to Freenet to browse suppliers, or enable example-data feature." }
                }
            }
        }
    }
}

fn example_suppliers() -> Vec<(String, String, String)> {
    vec![
        (
            "Green Valley Farm".into(),
            "Organic raw dairy from pastured cows".into(),
            "Portland, OR".into(),
        ),
        (
            "Mountain Creamery".into(),
            "Artisan cheese and butter".into(),
            "Burlington, VT".into(),
        ),
        (
            "Sunrise Dairy".into(),
            "Fresh raw milk and kefir".into(),
            "Lancaster, PA".into(),
        ),
    ]
}
