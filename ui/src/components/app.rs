use dioxus::prelude::*;

use super::directory_view::DirectoryView;
use super::my_orders::MyOrders;
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

    let user_state = use_user_state();
    let mut current_view = use_signal(|| View::Directory);

    // If no moniker set yet, show the setup screen
    if user_state.read().moniker.is_none() {
        return rsx! { MonikerSetup {} };
    }

    let moniker = user_state.read().moniker.clone().unwrap_or_default();
    let order_count = user_state.read().orders.len();

    rsx! {
        div { class: "cream-app",
            header { class: "app-header",
                div { class: "header-top",
                    h1 { "CREAM" }
                    span { class: "user-moniker", "Hi, {moniker}" }
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
                    button {
                        onclick: move |_| current_view.set(View::SupplierDashboard),
                        "Supplier Dashboard"
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
                    View::SupplierDashboard => rsx! { SupplierDashboard {} },
                    View::Wallet => rsx! { WalletView {} },
                }
            }
        }
    }
}

#[component]
fn MonikerSetup() -> Element {
    let mut user_state = use_user_state();
    let mut name_input = use_signal(|| String::new());

    rsx! {
        div { class: "cream-app",
            div { class: "moniker-setup",
                h1 { "Welcome to CREAM" }
                p { "CURD Retail Exchange And Marketplace" }
                p { "Choose a name to get started:" }
                div { class: "form-group",
                    input {
                        r#type: "text",
                        placeholder: "Your name or moniker...",
                        value: "{name_input}",
                        oninput: move |evt| name_input.set(evt.value()),
                        onkeypress: move |evt| {
                            if evt.key() == Key::Enter {
                                let name = name_input.read().trim().to_string();
                                if !name.is_empty() {
                                    user_state.write().moniker = Some(name);
                                }
                            }
                        },
                    }
                    button {
                        disabled: name_input.read().trim().is_empty(),
                        onclick: move |_| {
                            let name = name_input.read().trim().to_string();
                            if !name.is_empty() {
                                user_state.write().moniker = Some(name);
                            }
                        },
                        "Enter"
                    }
                }
            }
        }
    }
}
