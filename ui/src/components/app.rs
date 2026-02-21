use dioxus::prelude::*;

use cream_common::postcode::{format_postcode, is_valid_au_postcode};

use super::directory_view::DirectoryView;
use super::key_manager::KeyManager;
use super::my_orders::MyOrders;
#[cfg(feature = "use-node")]
use super::node_api::{use_node_action, NodeAction};
use super::node_api::use_node_coroutine;
use super::shared_state::SharedState;
use super::storefront_view::StorefrontView;
use super::supplier_dashboard::SupplierDashboard;
use super::user_state::{use_user_state, UserState};
use super::wallet_view::WalletView;

/// Normalize a name to title case: "gary" → "Gary", "GARY" → "Gary".
fn title_case(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap().to_uppercase().to_string();
    first + &chars.as_str().to_lowercase()
}

#[derive(Clone, Debug, PartialEq, Routable)]
pub enum Route {
    #[layout(AppLayout)]
    #[route("/directory")]
    Directory {},
    #[route("/supplier/:name")]
    Supplier { name: String },
    #[route("/orders")]
    Orders {},
    #[route("/my_storefront")]
    Dashboard {},
    #[route("/wallet")]
    Wallet {},
    #[redirect("/", || Route::Directory {})]
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

#[component]
pub fn App() -> Element {
    use_context_provider(|| Signal::new(UserState::new()));
    use_context_provider(|| Signal::new(SharedState::new()));
    // Try to auto-derive KeyManager from sessionStorage credentials
    use_context_provider(|| {
        let km = auto_derive_key_manager();
        Signal::new(km)
    });
    use_node_coroutine();

    let key_manager: Signal<Option<KeyManager>> = use_context();
    let user_state = use_user_state();

    // Not authenticated → show setup screen
    if key_manager.read().is_none() {
        return rsx! { SetupScreen {} };
    }

    // Authenticated but no profile yet → show setup
    if user_state.read().moniker.is_none() {
        return rsx! { SetupScreen {} };
    }

    rsx! { Router::<Route> {} }
}

/// Try to derive KeyManager from credentials stored in sessionStorage.
/// Returns Some(km) if moniker + password are available, None otherwise.
fn auto_derive_key_manager() -> Option<KeyManager> {
    let user_state = UserState::new();
    let moniker = user_state.moniker.as_deref()?;
    let password = UserState::load_password()?;
    KeyManager::from_credentials(moniker, &password).ok()
}

#[component]
fn AppLayout() -> Element {
    let mut user_state = use_user_state();
    let mut key_manager: Signal<Option<KeyManager>> = use_context();
    let nav = use_navigator();

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
                        a {
                            href: "#",
                            class: "logout-link",
                            onclick: move |_| {
                                UserState::clear_session();
                                key_manager.set(None);
                                user_state.set(UserState::new());
                            },
                            "(log out)"
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
        rsx! { DirectoryView {} }
    }
}

/// Route component: renders the wallet view.
#[component]
fn Wallet() -> Element {
    rsx! { WalletView {} }
}

/// Catch-all for unknown routes — redirects to directory.
#[component]
fn NotFound(segments: Vec<String>) -> Element {
    let nav = use_navigator();
    nav.push(Route::Directory {});
    rsx! {}
}

// ─── Setup ───────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum SetupStep {
    Profile,
    SetPassword,
}

#[component]
fn SetupScreen() -> Element {
    let mut key_manager: Signal<Option<KeyManager>> = use_context();
    let mut user_state = use_user_state();

    let mut step = use_signal(|| SetupStep::Profile);
    let mut name_input = use_signal(String::new);
    let mut postcode_input = use_signal(String::new);
    let mut is_supplier = use_signal(|| false);
    let mut supplier_desc = use_signal(String::new);
    let mut postcode_error = use_signal(|| None::<String>);

    let mut password = use_signal(String::new);
    let mut password_confirm = use_signal(String::new);
    let mut password_error = use_signal(|| None::<String>);
    let mut setup_error = use_signal(|| None::<String>);

    #[cfg(feature = "use-node")]
    let node = use_node_action();

    let current_step = step.read().clone();
    match &current_step {
        SetupStep::Profile => {
            let can_submit = {
                let name_ok = !name_input.read().trim().is_empty();
                let postcode_ok = is_valid_au_postcode(postcode_input.read().trim());
                let supplier_ok = !*is_supplier.read() || !supplier_desc.read().trim().is_empty();
                name_ok && postcode_ok && supplier_ok
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
                                    if val.trim().is_empty() || is_valid_au_postcode(val.trim()) {
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

                        if let Some(err) = setup_error.read().as_ref() {
                            p { class: "field-error", "{err}" }
                        }

                        button {
                            disabled: !can_submit,
                            onclick: move |_| {
                                step.set(SetupStep::SetPassword);
                            },
                            "Next"
                        }
                    }
                }
            }
        }

        SetupStep::SetPassword => {
            let pw_len = password.read().len();
            let pw_match = *password.read() == *password_confirm.read();
            let can_submit = pw_len >= 1 && pw_match;

            rsx! {
                div { class: "cream-app",
                    div { class: "user-setup",
                        h1 { "Set a Password" }
                        p { "This password, combined with your name, generates your identity. Use the same name and password to log in again." }

                        div { class: "form-group",
                            label { "Password:" }
                            input {
                                r#type: "password",
                                placeholder: "Enter password...",
                                value: "{password}",
                                oninput: move |evt| {
                                    password.set(evt.value());
                                    password_error.set(None);
                                },
                            }
                        }

                        div { class: "form-group",
                            label { "Confirm password:" }
                            input {
                                r#type: "password",
                                placeholder: "Confirm password...",
                                value: "{password_confirm}",
                                oninput: move |evt| {
                                    password_confirm.set(evt.value());
                                    password_error.set(None);
                                },
                            }
                            if !password_confirm.read().is_empty() && *password.read() != *password_confirm.read() {
                                span { class: "field-error", "Passwords do not match" }
                            }
                        }

                        if let Some(err) = password_error.read().as_ref() {
                            p { class: "field-error", "{err}" }
                        }
                        if let Some(err) = setup_error.read().as_ref() {
                            p { class: "field-error", "{err}" }
                        }

                        button {
                            disabled: !can_submit,
                            onclick: move |_| {
                                let pw = password.read().clone();
                                let pw2 = password_confirm.read().clone();
                                if pw != pw2 {
                                    password_error.set(Some("Passwords do not match".into()));
                                    return;
                                }

                                let name = title_case(&name_input.read());

                                let km = match KeyManager::from_credentials(&name, &pw) {
                                    Ok(km) => km,
                                    Err(e) => {
                                        setup_error.set(Some(format!("{e}")));
                                        return;
                                    }
                                };

                                let postcode = postcode_input.read().trim().to_string();
                                let is_sup = *is_supplier.read();
                                let desc = supplier_desc.read().trim().to_string();

                                {
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
                                }

                                // Save password to sessionStorage for auto-login on refresh
                                UserState::save_password(&pw);

                                key_manager.set(Some(km));

                                #[cfg(feature = "use-node")]
                                if is_sup {
                                    node.send(NodeAction::RegisterSupplier {
                                        name,
                                        postcode,
                                        description: desc,
                                    });
                                }
                            },
                            "Get Started"
                        }
                    }
                }
            }
        }
    }
}
