use dioxus::prelude::*;

use cream_common::postcode::{format_postcode, is_valid_au_postcode};

use super::directory_view::DirectoryView;
use super::my_orders::MyOrders;
#[cfg(feature = "use-node")]
use super::node_api::{use_node_action, NodeAction};
use super::node_api::use_node_coroutine;
use super::shared_state::SharedState;
use super::supplier_dashboard::SupplierDashboard;
use super::user_state::{use_user_state, UserState};
use super::wallet_view::WalletView;

#[derive(Clone, PartialEq)]
enum View {
    Directory,
    MyOrders,
    SupplierDashboard,
    Wallet,
}

#[component]
pub fn App() -> Element {
    // Provide shared user state to all child components
    use_context_provider(|| Signal::new(UserState::new()));
    use_context_provider(|| Signal::new(SharedState::new()));

    // Start node communication coroutine
    use_node_coroutine();

    let user_state = use_user_state();
    let mut current_view = use_signal(|| View::Directory);

    // If setup not complete, show the setup screen
    if user_state.read().moniker.is_none() {
        return rsx! { UserSetup {} };
    }

    let state = user_state.read();
    let moniker = state.moniker.clone().unwrap_or_default();
    let postcode_raw = state.postcode.clone().unwrap_or_default();
    let postcode_display = format_postcode(&postcode_raw);
    let order_count = state.orders.len();
    let is_supplier = state.is_supplier;
    drop(state);

    rsx! {
        div { class: "cream-app",
            header { class: "app-header",
                div { class: "header-top",
                    h1 { "CREAM" }
                    div { class: "user-info",
                        span { class: "user-moniker", "{moniker}" }
                        span { class: "user-postcode", " - {postcode_display}" }
                        if is_supplier {
                            span { class: "supplier-badge", " [Supplier]" }
                        }
                    }
                }
                p { "CURD Retail Exchange And Marketplace" }
                nav {
                    button {
                        onclick: move |_| current_view.set(View::Directory),
                        "Browse Suppliers"
                    }
                    button {
                        onclick: move |_| current_view.set(View::MyOrders),
                        "My Orders ({order_count})"
                    }
                    if is_supplier {
                        button {
                            onclick: move |_| current_view.set(View::SupplierDashboard),
                            "My Storefront"
                        }
                    }
                    button {
                        onclick: move |_| current_view.set(View::Wallet),
                        "Wallet"
                    }
                }
            }
            main {
                match *current_view.read() {
                    View::Directory => rsx! { DirectoryView {} },
                    View::MyOrders => rsx! { MyOrders {} },
                    View::SupplierDashboard if is_supplier => rsx! { SupplierDashboard {} },
                    View::SupplierDashboard => rsx! { DirectoryView {} },
                    View::Wallet => rsx! { WalletView {} },
                }
            }
        }
    }
}

#[component]
fn UserSetup() -> Element {
    let mut user_state = use_user_state();
    let mut name_input = use_signal(|| String::new());
    let mut postcode_input = use_signal(|| String::new());
    let mut is_supplier = use_signal(|| false);
    let mut supplier_desc = use_signal(|| String::new());
    let mut postcode_error = use_signal(|| None::<String>);

    let can_submit = use_memo(move || {
        let name_ok = !name_input.read().trim().is_empty();
        let postcode_ok = is_valid_au_postcode(postcode_input.read().trim());
        let supplier_ok = !*is_supplier.read() || !supplier_desc.read().trim().is_empty();
        name_ok && postcode_ok && supplier_ok
    });

    let submit = move |_| {
        let name = name_input.read().trim().to_string();
        let postcode = postcode_input.read().trim().to_string();

        if name.is_empty() {
            return;
        }
        if !is_valid_au_postcode(&postcode) {
            postcode_error.set(Some("Invalid Australian postcode".into()));
            return;
        }

        let is_sup = *is_supplier.read();
        let desc = supplier_desc.read().trim().to_string();

        let mut state = user_state.write();
        state.moniker = Some(name.clone());
        state.postcode = Some(postcode.clone());
        state.is_supplier = is_sup;
        if is_sup {
            state.supplier_description = if desc.is_empty() {
                None
            } else {
                Some(desc.clone())
            };
        }
        drop(state);

        // Register supplier with the Freenet network
        #[cfg(feature = "use-node")]
        if is_sup {
            let node = use_node_action();
            node.send(NodeAction::RegisterSupplier {
                name,
                postcode,
                description: desc,
            });
        }
    };

    rsx! {
        div { class: "cream-app",
            div { class: "user-setup",
                h1 { "Welcome to CREAM" }
                p { "CURD Retail Exchange And Marketplace" }

                div { class: "form-group",
                    label { "Your name:" }
                    input {
                        r#type: "text",
                        placeholder: "Name or moniker...",
                        value: "{name_input}",
                        oninput: move |evt| name_input.set(evt.value()),
                    }
                }

                div { class: "form-group",
                    label { "Postcode (Australia):" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. 2000",
                        maxlength: "4",
                        value: "{postcode_input}",
                        oninput: move |evt| {
                            let val = evt.value();
                            postcode_input.set(val.clone());
                            if val.trim().is_empty() {
                                postcode_error.set(None);
                            } else if is_valid_au_postcode(val.trim()) {
                                postcode_error.set(None);
                            } else {
                                postcode_error.set(Some("Not a recognised postcode".into()));
                            }
                        },
                    }
                    if let Some(err) = postcode_error.read().as_ref() {
                        span { class: "field-error", "{err}" }
                    }
                }

                div { class: "form-group",
                    label {
                        input {
                            r#type: "checkbox",
                            checked: *is_supplier.read(),
                            onchange: move |evt| is_supplier.set(evt.checked()),
                        }
                        " I want to sell products (register as supplier)"
                    }
                }

                if *is_supplier.read() {
                    div { class: "form-group",
                        label { "Storefront description:" }
                        textarea {
                            placeholder: "Describe your farm or dairy...",
                            value: "{supplier_desc}",
                            oninput: move |evt| supplier_desc.set(evt.value()),
                        }
                    }
                }

                button {
                    disabled: !can_submit(),
                    onclick: submit,
                    "Get Started"
                }
            }
        }
    }
}
