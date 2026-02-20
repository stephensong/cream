use dioxus::prelude::*;

use cream_common::postcode::{format_postcode, is_valid_au_postcode};

use super::directory_view::DirectoryView;
use super::my_orders::MyOrders;
#[cfg(feature = "use-node")]
use super::node_api::{use_node_action, NodeAction};
use super::node_api::use_node_coroutine;
use super::shared_state::SharedState;
use super::storefront_view::StorefrontView;
use super::supplier_dashboard::SupplierDashboard;
use super::user_state::{use_user_state, UserState};
use super::wallet_view::WalletView;

#[derive(Clone, Debug, PartialEq, Routable)]
pub enum Route {
    #[layout(AppLayout)]
    #[route("/")]
    Directory {},
    #[route("/supplier/:name")]
    Supplier { name: String },
    #[route("/orders")]
    Orders {},
    #[route("/dashboard")]
    Dashboard {},
    #[route("/wallet")]
    Wallet {},
    #[end_layout]
    #[route("/setup")]
    Setup {},
}

#[component]
pub fn App() -> Element {
    use_context_provider(|| Signal::new(UserState::new()));
    use_context_provider(|| Signal::new(SharedState::new()));
    use_node_coroutine();

    rsx! { Router::<Route> {} }
}

#[component]
fn AppLayout() -> Element {
    let user_state = use_user_state();
    let nav = use_navigator();

    // Redirect to setup if no moniker
    if user_state.read().moniker.is_none() {
        nav.replace(Route::Setup {});
        return rsx! {};
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
                        onclick: move |_| { nav.push(Route::Directory {}); },
                        "Browse Suppliers"
                    }
                    button {
                        onclick: move |_| { nav.push(Route::Orders {}); },
                        "My Orders ({order_count})"
                    }
                    if is_supplier {
                        button {
                            onclick: move |_| { nav.push(Route::Dashboard {}); },
                            "My Storefront"
                        }
                    }
                    button {
                        onclick: move |_| { nav.push(Route::Wallet {}); },
                        "Wallet"
                    }
                }
            }
            main {
                Outlet::<Route> {}
            }
        }
    }
}

/// Route component: renders the directory view.
#[component]
fn Directory() -> Element {
    rsx! { DirectoryView {} }
}

/// Route component: renders a supplier's storefront by name from the URL.
#[component]
fn Supplier(name: String) -> Element {
    rsx! { StorefrontView { supplier_name: name } }
}

/// Route component: renders the orders view.
#[component]
fn Orders() -> Element {
    rsx! { MyOrders {} }
}

/// Route component: renders the supplier dashboard.
#[component]
fn Dashboard() -> Element {
    let user_state = use_user_state();
    let is_supplier = user_state.read().is_supplier;

    if is_supplier {
        rsx! { SupplierDashboard {} }
    } else {
        // Non-suppliers who navigate here get redirected to directory
        rsx! { DirectoryView {} }
    }
}

/// Route component: renders the wallet view.
#[component]
fn Wallet() -> Element {
    rsx! { WalletView {} }
}

#[component]
fn Setup() -> Element {
    rsx! { UserSetup {} }
}

#[component]
fn UserSetup() -> Element {
    let mut user_state = use_user_state();
    let nav = use_navigator();
    let mut name_input = use_signal(|| String::new());
    let mut postcode_input = use_signal(|| String::new());
    let mut is_supplier = use_signal(|| false);
    let mut supplier_desc = use_signal(|| String::new());
    let mut postcode_error = use_signal(|| None::<String>);

    #[cfg(feature = "use-node")]
    let node = use_node_action();

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
        state.save();
        drop(state);

        // Register supplier with the Freenet network
        #[cfg(feature = "use-node")]
        if is_sup {
            node.send(NodeAction::RegisterSupplier {
                name,
                postcode,
                description: desc,
            });
        }

        // Navigate to directory after setup
        nav.replace(Route::Directory {});
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
