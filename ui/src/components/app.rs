use dioxus::prelude::*;

use cream_common::postcode::{format_postcode, is_valid_au_postcode};

use super::directory_view::DirectoryView;
use super::key_manager::{KeyManager, KeyManagerError};
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

/// Which auth screen should we show when the user is not logged in?
#[derive(Clone, PartialEq)]
enum AuthScreen {
    Login,
    Setup,
    Recover,
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
    use_context_provider(|| Signal::new(None::<KeyManager>));
    use_node_coroutine();

    let key_manager: Signal<Option<KeyManager>> = use_context();
    let user_state = use_user_state();

    // Determine initial auth screen
    let initial_screen = if KeyManager::has_stored_identity() {
        AuthScreen::Login
    } else {
        AuthScreen::Setup
    };
    let mut auth_screen = use_signal(|| initial_screen);

    // Not authenticated → show auth screen
    if key_manager.read().is_none() {
        let screen = auth_screen.read().clone();
        return match screen {
            AuthScreen::Login => rsx! {
                LoginScreen { on_switch: move |s| auth_screen.set(s) }
            },
            AuthScreen::Setup => rsx! {
                SetupScreen { on_switch: move |s| auth_screen.set(s) }
            },
            AuthScreen::Recover => rsx! {
                RecoverScreen { on_switch: move |s| auth_screen.set(s) }
            },
        };
    }

    // Authenticated but no profile yet → show setup
    if user_state.read().moniker.is_none() {
        return rsx! {
            SetupScreen { on_switch: move |s| auth_screen.set(s) }
        };
    }

    rsx! { Router::<Route> {} }
}

#[component]
fn AppLayout() -> Element {
    let user_state = use_user_state();
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

// ─── Login ───────────────────────────────────────────────────────────────────

#[component]
fn LoginScreen(on_switch: EventHandler<AuthScreen>) -> Element {
    let mut key_manager: Signal<Option<KeyManager>> = use_context();
    let user_state = use_user_state();
    let mut password = use_signal(|| String::new());
    let mut error = use_signal(|| None::<String>);

    let moniker = user_state.read().moniker.clone().unwrap_or_default();

    rsx! {
        div { class: "cream-app",
            div { class: "user-setup",
                h1 { "Welcome Back" }
                if !moniker.is_empty() {
                    p { "Unlock your identity as {moniker}." }
                } else {
                    p { "Enter your password to unlock your identity." }
                }

                div { class: "form-group",
                    label { "Password:" }
                    input {
                        r#type: "password",
                        placeholder: "Enter password...",
                        value: "{password}",
                        oninput: move |evt| {
                            password.set(evt.value());
                            error.set(None);
                        },
                    }
                }

                if let Some(err) = error.read().as_ref() {
                    p { class: "field-error", "{err}" }
                }

                button {
                    disabled: password.read().is_empty(),
                    onclick: move |_| {
                        let pw = password.read().clone();
                        match KeyManager::unlock(&pw) {
                            Ok(km) => {
                                key_manager.set(Some(km));
                            }
                            Err(KeyManagerError::DecryptionFailed) => {
                                error.set(Some("Wrong password. Please try again.".into()));
                            }
                            Err(e) => {
                                error.set(Some(format!("{e}")));
                            }
                        }
                    },
                    "Unlock"
                }

                p { class: "alt-action",
                    a {
                        href: "#",
                        onclick: move |_| {
                            KeyManager::clear_stored_identity();
                            #[cfg(target_family = "wasm")]
                            if let Some(storage) = web_sys::window()
                                .and_then(|w| w.local_storage().ok())
                                .flatten()
                            {
                                let _ = storage.remove_item("cream_user_state");
                            }
                            on_switch.call(AuthScreen::Setup);
                        },
                        "Use a different identity"
                    }
                }
            }
        }
    }
}

// ─── Setup (multi-step) ─────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum SetupStep {
    Profile,
    DisplayMnemonic,
    SetPassword,
}

#[component]
fn SetupScreen(on_switch: EventHandler<AuthScreen>) -> Element {
    let mut key_manager: Signal<Option<KeyManager>> = use_context();
    let mut user_state = use_user_state();

    let mut step = use_signal(|| SetupStep::Profile);
    let mut name_input = use_signal(|| String::new());
    let mut postcode_input = use_signal(|| String::new());
    let mut is_supplier = use_signal(|| false);
    let mut supplier_desc = use_signal(|| String::new());
    let mut postcode_error = use_signal(|| None::<String>);

    let mut mnemonic = use_signal(|| None::<bip39::Mnemonic>);
    let mut saved_confirmed = use_signal(|| false);

    let mut password = use_signal(|| String::new());
    let mut password_confirm = use_signal(|| String::new());
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

                        if let Some(err) = setup_error.read().as_ref() {
                            p { class: "field-error", "{err}" }
                        }

                        button {
                            disabled: !can_submit,
                            onclick: move |_| {
                                let name = name_input.read().trim().to_string();
                                let postcode = postcode_input.read().trim().to_string();
                                if name.is_empty() { return; }
                                if !is_valid_au_postcode(&postcode) {
                                    postcode_error.set(Some("Invalid Australian postcode".into()));
                                    return;
                                }
                                match KeyManager::generate_mnemonic() {
                                    Ok(m) => {
                                        mnemonic.set(Some(m));
                                        step.set(SetupStep::DisplayMnemonic);
                                    }
                                    Err(e) => {
                                        setup_error.set(Some(format!("{e}")));
                                    }
                                }
                            },
                            "Next"
                        }

                        p { class: "alt-action",
                            a {
                                href: "#",
                                onclick: move |_| on_switch.call(AuthScreen::Recover),
                                "Recover existing identity"
                            }
                        }
                    }
                }
            }
        }

        SetupStep::DisplayMnemonic => {
            let words: Vec<String> = mnemonic
                .read()
                .as_ref()
                .map(|m| m.words().map(|w| w.to_string()).collect())
                .unwrap_or_default();

            rsx! {
                div { class: "cream-app",
                    div { class: "user-setup",
                        h1 { "Save Your Recovery Phrase" }
                        p { "Write down these 12 words in order. You will need them to recover your identity on a new device." }
                        p { strong { "Do not share these words with anyone." } }

                        div { class: "mnemonic-grid",
                            for (i, word) in words.iter().enumerate() {
                                span { class: "mnemonic-word",
                                    span { class: "word-number", "{i + 1}. " }
                                    "{word}"
                                }
                            }
                        }

                        div { class: "form-group",
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: *saved_confirmed.read(),
                                    onchange: move |evt| saved_confirmed.set(evt.checked()),
                                }
                                " I have saved these words in a secure location"
                            }
                        }

                        button {
                            disabled: !*saved_confirmed.read(),
                            onclick: move |_| { step.set(SetupStep::SetPassword); },
                            "Continue"
                        }
                    }
                }
            }
        }

        SetupStep::SetPassword => {
            let pw_len = password.read().len();
            let pw_match = *password.read() == *password_confirm.read();
            let can_submit = pw_len >= 8 && pw_match;

            rsx! {
                div { class: "cream-app",
                    div { class: "user-setup",
                        h1 { "Set a Password" }
                        p { "This password encrypts your identity in this browser. You will use it to log in next time." }

                        div { class: "form-group",
                            label { "Password (min 8 characters):" }
                            input {
                                r#type: "password",
                                placeholder: "Enter password...",
                                value: "{password}",
                                oninput: move |evt| {
                                    password.set(evt.value());
                                    password_error.set(None);
                                },
                            }
                            if !password.read().is_empty() && password.read().len() < 8 {
                                {
                                    let remaining = 8 - password.read().len();
                                    let s = if remaining == 1 { "" } else { "s" };
                                    rsx! { span { class: "field-error", "{remaining} more character{s} needed" } }
                                }
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
                                if pw.len() < 8 {
                                    password_error.set(Some("Password must be at least 8 characters".into()));
                                    return;
                                }
                                if pw != pw2 {
                                    password_error.set(Some("Passwords do not match".into()));
                                    return;
                                }

                                let m = mnemonic.read().clone();
                                let Some(m) = m else {
                                    setup_error.set(Some("Mnemonic lost — please restart setup".into()));
                                    return;
                                };

                                let km = match KeyManager::from_mnemonic(&m) {
                                    Ok(km) => km,
                                    Err(e) => {
                                        setup_error.set(Some(format!("{e}")));
                                        return;
                                    }
                                };

                                if let Err(e) = KeyManager::save_encrypted(&m, &pw) {
                                    setup_error.set(Some(format!("{e}")));
                                    return;
                                }

                                let name = title_case(&name_input.read());
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

// ─── Recover ─────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum RecoverStep {
    EnterMnemonic,
    Profile,
    SetPassword,
}

#[component]
fn RecoverScreen(on_switch: EventHandler<AuthScreen>) -> Element {
    let mut key_manager: Signal<Option<KeyManager>> = use_context();
    let mut user_state = use_user_state();

    let mut step = use_signal(|| RecoverStep::EnterMnemonic);
    let mut mnemonic_input = use_signal(|| String::new());
    let mut mnemonic = use_signal(|| None::<bip39::Mnemonic>);
    let mut mnemonic_error = use_signal(|| None::<String>);

    let mut name_input = use_signal(|| String::new());
    let mut postcode_input = use_signal(|| String::new());
    let mut is_supplier = use_signal(|| false);
    let mut supplier_desc = use_signal(|| String::new());
    let mut postcode_error = use_signal(|| None::<String>);

    let mut password = use_signal(|| String::new());
    let mut password_confirm = use_signal(|| String::new());
    let mut password_error = use_signal(|| None::<String>);
    let mut setup_error = use_signal(|| None::<String>);

    #[cfg(feature = "use-node")]
    let node = use_node_action();

    let current_step = step.read().clone();
    match &current_step {
        RecoverStep::EnterMnemonic => {
            rsx! {
                div { class: "cream-app",
                    div { class: "user-setup",
                        h1 { "Recover Identity" }
                        p { "Enter your 12-word recovery phrase to restore your identity." }

                        div { class: "form-group",
                            label { "Recovery phrase:" }
                            textarea {
                                placeholder: "Enter your 12 words separated by spaces...",
                                rows: "3",
                                value: "{mnemonic_input}",
                                oninput: move |evt| {
                                    mnemonic_input.set(evt.value());
                                    mnemonic_error.set(None);
                                },
                            }
                        }

                        if let Some(err) = mnemonic_error.read().as_ref() {
                            p { class: "field-error", "{err}" }
                        }

                        button {
                            disabled: mnemonic_input.read().trim().is_empty(),
                            onclick: move |_| {
                                let input = mnemonic_input.read().trim().to_string();
                                match bip39::Mnemonic::parse_in(bip39::Language::English, &input) {
                                    Ok(m) => {
                                        mnemonic.set(Some(m));
                                        mnemonic_error.set(None);
                                        step.set(RecoverStep::Profile);
                                    }
                                    Err(_) => {
                                        mnemonic_error.set(Some(
                                            "Invalid mnemonic. Please enter exactly 12 valid BIP39 words.".into(),
                                        ));
                                    }
                                }
                            },
                            "Next"
                        }

                        p { class: "alt-action",
                            a {
                                href: "#",
                                onclick: move |_| on_switch.call(AuthScreen::Setup),
                                "Create new identity instead"
                            }
                        }
                    }
                }
            }
        }

        RecoverStep::Profile => {
            let can_submit = {
                let name_ok = !name_input.read().trim().is_empty();
                let postcode_ok = is_valid_au_postcode(postcode_input.read().trim());
                let supplier_ok = !*is_supplier.read() || !supplier_desc.read().trim().is_empty();
                name_ok && postcode_ok && supplier_ok
            };

            rsx! {
                div { class: "cream-app",
                    div { class: "user-setup",
                        h1 { "Set Up Your Profile" }
                        p { "Your identity has been verified. Now set up your profile." }

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
                            disabled: !can_submit,
                            onclick: move |_| { step.set(RecoverStep::SetPassword); },
                            "Next"
                        }
                    }
                }
            }
        }

        RecoverStep::SetPassword => {
            let pw_len = password.read().len();
            let pw_match = *password.read() == *password_confirm.read();
            let can_submit = pw_len >= 8 && pw_match;

            rsx! {
                div { class: "cream-app",
                    div { class: "user-setup",
                        h1 { "Set a Password" }
                        p { "This password encrypts your identity in this browser." }

                        div { class: "form-group",
                            label { "Password (min 8 characters):" }
                            input {
                                r#type: "password",
                                placeholder: "Enter password...",
                                value: "{password}",
                                oninput: move |evt| {
                                    password.set(evt.value());
                                    password_error.set(None);
                                },
                            }
                            if !password.read().is_empty() && password.read().len() < 8 {
                                {
                                    let remaining = 8 - password.read().len();
                                    let s = if remaining == 1 { "" } else { "s" };
                                    rsx! { span { class: "field-error", "{remaining} more character{s} needed" } }
                                }
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
                                if pw.len() < 8 {
                                    password_error.set(Some("Password must be at least 8 characters".into()));
                                    return;
                                }
                                if pw != pw2 {
                                    password_error.set(Some("Passwords do not match".into()));
                                    return;
                                }

                                let m = mnemonic.read().clone();
                                let Some(m) = m else {
                                    setup_error.set(Some("Mnemonic lost — please restart".into()));
                                    return;
                                };

                                let km = match KeyManager::from_mnemonic(&m) {
                                    Ok(km) => km,
                                    Err(e) => {
                                        setup_error.set(Some(format!("{e}")));
                                        return;
                                    }
                                };

                                if let Err(e) = KeyManager::save_encrypted(&m, &pw) {
                                    setup_error.set(Some(format!("{e}")));
                                    return;
                                }

                                let name = title_case(&name_input.read());
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
