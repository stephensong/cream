use dioxus::prelude::*;
use dioxus_router::Navigator;

use cream_common::postcode::{
    format_postcode, is_valid_au_postcode, lookup_all_localities, PostcodeInfo,
};

use super::directory_view::DirectoryView;
use super::key_manager::KeyManager;
use super::my_orders::MyOrders;
#[cfg(feature = "use-node")]
use super::node_api::{use_node_action, NodeAction};
use super::node_api::use_node_coroutine;
use super::shared_state::{use_shared_state, SharedState};
use super::storefront_view::StorefrontView;
use super::supplier_dashboard::SupplierDashboard;
use super::user_state::{use_user_state, UserState};
use super::wallet_view::WalletView;

/// Read `?supplier=X` from the browser URL bar. Returns `None` outside WASM.
fn get_supplier_query_param() -> Option<String> {
    #[cfg(target_family = "wasm")]
    {
        let search = web_sys::window()?.location().search().ok()?;
        let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
        params.get("supplier").filter(|s| !s.is_empty())
    }
    #[cfg(not(target_family = "wasm"))]
    {
        None
    }
}

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

    let setup_needed = key_manager.read().is_none() || user_state.read().moniker.is_none();

    let content = if setup_needed {
        rsx! { SetupScreen {} }
    } else {
        rsx! { Router::<Route> {} }
    };

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }
        {content}
    }
}

/// Render the navigation buttons for the app header.
fn nav_buttons(nav: Navigator, order_count: usize, displayed_balance: u64, is_supplier: bool, connected_supplier: Option<String>) -> Element {
    if let Some(supplier) = connected_supplier {
        // Customer mode: single-storefront nav
        rsx! {
            nav {
                {
                    let supplier_clone = supplier.clone();
                    rsx! {
                        button {
                            onclick: move |_| { nav.push(Route::Supplier { name: supplier_clone.clone() }); },
                            "Storefront"
                        }
                    }
                }
                button {
                    onclick: move |_| { nav.push(Route::Orders {}); },
                    "My Orders ({order_count})"
                }
                button {
                    onclick: move |_| { nav.push(Route::Wallet {}); },
                    "Wallet ({displayed_balance} CURD)"
                }
            }
        }
    } else {
        // Supplier / browser mode: full directory nav
        rsx! {
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
                    "Wallet ({displayed_balance} CURD)"
                }
            }
        }
    }
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
    let locality = state.locality.clone();
    let postcode_display = format_postcode(&postcode_raw, locality.as_deref());
    let order_count = state.orders.len();
    let is_customer = state.connected_supplier.is_some();
    let is_supplier = state.is_supplier;
    let balance = state.balance;
    let connected_supplier = state.connected_supplier.clone();
    drop(state);

    // Compute displayed balance: base + incoming deposit credits for suppliers
    let incoming_deposits: u64 = if !is_customer && is_supplier {
        let shared = use_shared_state();
        let shared_read = shared.read();
        shared_read
            .storefronts
            .get(&moniker)
            .map(|sf| sf.orders.values().map(|o| o.deposit_amount).sum())
            .unwrap_or(0)
    } else {
        0
    };
    let displayed_balance = balance + incoming_deposits;

    rsx! {
        div { class: "cream-app",
            header { class: "app-header",
                div { class: "header-top",
                    h1 { "CREAM " span { class: "tagline", "rises to the top" } }
                    div { class: "user-info",
                        span { class: "user-moniker", "{moniker}" }
                        span { class: "user-postcode", " - {postcode_display}" }
                        if !is_customer && is_supplier {
                            span { class: "supplier-badge", " [Supplier]" }
                        }
                        button {
                            class: "logout-btn",
                            onclick: move |_| {
                                UserState::clear_session();
                                key_manager.set(None);
                                user_state.set(UserState::new());
                            },
                            "Log out"
                        }
                    }
                }
                p { "The decentralized, private 24/7 farmer's market" }
                {nav_buttons(nav.clone(), order_count, displayed_balance, is_supplier, connected_supplier.clone())}
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
    let user_state = use_user_state();
    let connected = user_state.read().connected_supplier.clone();
    if let Some(supplier) = connected {
        let nav = use_navigator();
        nav.push(Route::Supplier { name: supplier });
        rsx! {}
    } else {
        rsx! { DirectoryView {} }
    }
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
    let connected = user_state.read().connected_supplier.clone();
    if let Some(supplier) = connected {
        let nav = use_navigator();
        nav.push(Route::Supplier { name: supplier });
        rsx! {}
    } else {
        let is_supplier = user_state.read().is_supplier;

        if is_supplier {
            rsx! { SupplierDashboard {} }
        } else {
            rsx! { DirectoryView {} }
        }
    }
}

/// Route component: renders the wallet view.
#[component]
fn Wallet() -> Element {
    rsx! { WalletView {} }
}

/// Catch-all for unknown routes — redirects to directory (or storefront in customer mode).
#[component]
fn NotFound(segments: Vec<String>) -> Element {
    let nav = use_navigator();
    let user_state = use_user_state();
    if let Some(supplier) = user_state.read().connected_supplier.clone() {
        nav.push(Route::Supplier { name: supplier });
    } else {
        nav.push(Route::Directory {});
    }
    rsx! {}
}

// ─── Setup ───────────────────────────────────────────────────────────────────

#[component]
fn SetupScreen() -> Element {
    let mut key_manager: Signal<Option<KeyManager>> = use_context();
    let mut user_state = use_user_state();
    let _shared_state = use_shared_state();

    let mut name_input = use_signal(String::new);
    let mut postcode_input = use_signal(String::new);
    let mut is_supplier = use_signal(|| false);
    let mut supplier_desc = use_signal(String::new);
    let mut postcode_error = use_signal(|| None::<String>);

    // Locality selection
    let mut localities = use_signal(Vec::<PostcodeInfo>::new);
    let mut selected_locality = use_signal(|| None::<String>);

    // Returning user detection (supplier mode only)
    let mut welcome_back = use_signal(|| None::<String>);

    // Auto-connect via ?supplier= query param
    let url_supplier = use_signal(|| get_supplier_query_param());
    let auto_connect_mode = url_supplier.read().is_some();

    // Supplier name lookup (customer mode)
    let mut supplier_name_input = use_signal(|| {
        get_supplier_query_param().unwrap_or_default()
    });
    let mut supplier_lookup_error = use_signal(|| None::<String>);
    let mut supplier_lookup_loading = use_signal(|| false);
    let mut supplier_lookup_result =
        use_signal(|| None::<super::rendezvous::RendezvousEntry>);

    // Auto-trigger lookup when ?supplier= param is present
    use_effect(move || {
        if let Some(name) = url_supplier.read().clone() {
            supplier_lookup_loading.set(true);
            spawn(async move {
                match super::rendezvous::lookup_supplier(&name).await {
                    Ok(entry) => {
                        supplier_lookup_result.set(Some(entry));
                    }
                    Err(e) => {
                        supplier_lookup_error.set(Some(e));
                    }
                }
                supplier_lookup_loading.set(false);
            });
        }
    });

    let mut setup_error = use_signal(|| None::<String>);

    #[cfg(feature = "use-node")]
    let node = use_node_action();

    let can_submit = {
                let name_ok = !name_input.read().trim().is_empty();
                let postcode_ok = is_valid_au_postcode(postcode_input.read().trim());
                let locality_ok = localities.read().len() <= 1
                    || selected_locality.read().is_some();
                let supplier_ok = if auto_connect_mode {
                    // In auto-connect mode, need a successful lookup
                    supplier_lookup_result.read().is_some()
                } else {
                    !*is_supplier.read() || !supplier_desc.read().trim().is_empty()
                };
                name_ok && postcode_ok && locality_ok && supplier_ok
            };

            let localities_list = localities.read().clone();
            let current_locality = selected_locality.read().clone();
            let welcome_msg = welcome_back.read().clone();

            rsx! {
                div { class: "cream-app",
                    div { class: "user-setup",
                        h1 { "Welcome to CREAM" }
                        p { "The decentralized, private 24/7 farmer's market" }

                        div { class: "form-group",
                            label { "Your name:" }
                            input {
                                r#type: "text",
                                placeholder: "Name or moniker...",
                                value: "{name_input}",
                                oninput: move |evt| {
                                    let val = evt.value();
                                    name_input.set(val.clone());

                                    // Check directory for returning supplier
                                    let canonical = title_case(&val);
                                    let shared = _shared_state.read();
                                    let found = shared.directory.entries.values().find(|e| {
                                        e.name.eq_ignore_ascii_case(canonical.trim())
                                    });
                                    if let Some(entry) = found {
                                        is_supplier.set(true);
                                        supplier_desc.set(entry.description.clone());
                                        if let Some(pc) = entry.postcode.as_deref() {
                                            postcode_input.set(pc.to_string());
                                            let locs = lookup_all_localities(pc);
                                            if locs.len() == 1 {
                                                selected_locality.set(Some(locs[0].place_name.clone()));
                                            } else if let Some(loc) = entry.locality.as_deref() {
                                                // Auto-select matching locality
                                                if locs.iter().any(|l| l.place_name == loc) {
                                                    selected_locality.set(Some(loc.to_string()));
                                                } else {
                                                    selected_locality.set(None);
                                                }
                                            } else {
                                                selected_locality.set(None);
                                            }
                                            postcode_error.set(None);
                                            localities.set(locs);
                                        }
                                        welcome_back.set(Some(format!("Welcome back, {}!", entry.name)));
                                    } else {
                                        welcome_back.set(None);
                                    }
                                },
                            }
                        }

                        if let Some(msg) = welcome_msg.as_ref() {
                            p { class: "welcome-back", "{msg}" }
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
                                    let trimmed = val.trim();
                                    if trimmed.is_empty() {
                                        postcode_error.set(None);
                                        localities.set(Vec::new());
                                        selected_locality.set(None);
                                    } else if is_valid_au_postcode(trimmed) {
                                        postcode_error.set(None);
                                        let locs = lookup_all_localities(trimmed);
                                        if locs.len() == 1 {
                                            selected_locality.set(Some(locs[0].place_name.clone()));
                                        } else {
                                            selected_locality.set(None);
                                        }
                                        localities.set(locs);
                                    } else {
                                        postcode_error.set(Some("Not a recognised postcode".into()));
                                        localities.set(Vec::new());
                                        selected_locality.set(None);
                                    }
                                },
                            }
                            if let Some(err) = postcode_error.read().as_ref() {
                                span { class: "field-error", "{err}" }
                            }
                        }

                        // Locality selection (only when multiple localities for the postcode)
                        if localities_list.len() > 1 {
                            div { class: "form-group",
                                label { "Locality:" }
                                select {
                                    value: current_locality.as_deref().unwrap_or(""),
                                    onchange: move |evt| {
                                        let val = evt.value();
                                        if val.is_empty() {
                                            selected_locality.set(None);
                                        } else {
                                            selected_locality.set(Some(val));
                                        }
                                    },
                                    option { value: "", "Select a locality..." }
                                    for loc in &localities_list {
                                        option {
                                            value: "{loc.place_name}",
                                            selected: current_locality.as_deref() == Some(loc.place_name.as_str()),
                                            "{loc.place_name}"
                                        }
                                    }
                                }
                            }
                        } else if localities_list.len() == 1 {
                            div { class: "form-group",
                                label { "Locality:" }
                                span { class: "locality-auto", "{localities_list[0].place_name}" }
                            }
                        }

                        if auto_connect_mode {
                            // Auto-connect via ?supplier= URL param
                            div { class: "form-group",
                                if *supplier_lookup_loading.read() {
                                    p { "Connecting to supplier: {supplier_name_input}..." }
                                } else if let Some(entry) = supplier_lookup_result.read().as_ref() {
                                    p { class: "welcome-back",
                                        "Connected to: {entry.name}"
                                    }
                                } else if let Some(err) = supplier_lookup_error.read().as_ref() {
                                    span { class: "field-error", "{err}" }
                                    // Fall back to manual lookup on error
                                    div { class: "supplier-lookup-row",
                                        input {
                                            r#type: "text",
                                            placeholder: "e.g. garys-farm",
                                            value: "{supplier_name_input}",
                                            oninput: move |evt| {
                                                supplier_name_input.set(evt.value());
                                                supplier_lookup_error.set(None);
                                                supplier_lookup_result.set(None);
                                            },
                                        }
                                        button {
                                            disabled: supplier_name_input.read().trim().is_empty()
                                                || *supplier_lookup_loading.read(),
                                            onclick: move |_| {
                                                let name = supplier_name_input.read().trim().to_string();
                                                if name.is_empty() {
                                                    return;
                                                }
                                                supplier_lookup_loading.set(true);
                                                supplier_lookup_error.set(None);
                                                supplier_lookup_result.set(None);
                                                spawn(async move {
                                                    match super::rendezvous::lookup_supplier(&name).await {
                                                        Ok(entry) => {
                                                            supplier_lookup_result.set(Some(entry));
                                                        }
                                                        Err(e) => {
                                                            supplier_lookup_error.set(Some(e));
                                                        }
                                                    }
                                                    supplier_lookup_loading.set(false);
                                                });
                                            },
                                            if *supplier_lookup_loading.read() {
                                                "Looking up..."
                                            } else {
                                                "Look up"
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            // Normal flow: supplier checkbox + manual lookup
                            div { class: "form-group",
                                label {
                                    input {
                                        r#type: "checkbox",
                                        checked: *is_supplier.read(),
                                        onchange: move |evt| {
                                            is_supplier.set(evt.checked());
                                            if evt.checked() {
                                                // Clear supplier lookup state when switching to supplier mode
                                                supplier_name_input.set(String::new());
                                                supplier_lookup_error.set(None);
                                                supplier_lookup_result.set(None);
                                            }
                                        },
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

                            if !*is_supplier.read() {
                                div { class: "form-group",
                                    label { "Or connect to a specific supplier:" }
                                    div { class: "supplier-lookup-row",
                                        input {
                                            r#type: "text",
                                            placeholder: "e.g. garys-farm",
                                            value: "{supplier_name_input}",
                                            oninput: move |evt| {
                                                supplier_name_input.set(evt.value());
                                                supplier_lookup_error.set(None);
                                                supplier_lookup_result.set(None);
                                            },
                                        }
                                        button {
                                            disabled: supplier_name_input.read().trim().is_empty()
                                                || *supplier_lookup_loading.read(),
                                            onclick: move |_| {
                                                let name = supplier_name_input.read().trim().to_string();
                                                if name.is_empty() {
                                                    return;
                                                }
                                                supplier_lookup_loading.set(true);
                                                supplier_lookup_error.set(None);
                                                supplier_lookup_result.set(None);
                                                spawn(async move {
                                                    match super::rendezvous::lookup_supplier(&name).await {
                                                        Ok(entry) => {
                                                            supplier_lookup_result.set(Some(entry));
                                                        }
                                                        Err(e) => {
                                                            supplier_lookup_error.set(Some(e));
                                                        }
                                                    }
                                                    supplier_lookup_loading.set(false);
                                                });
                                            },
                                            if *supplier_lookup_loading.read() {
                                                "Looking up..."
                                            } else {
                                                "Look up"
                                            }
                                        }
                                    }
                                    if let Some(err) = supplier_lookup_error.read().as_ref() {
                                        span { class: "field-error", "{err}" }
                                    }
                                    if let Some(entry) = supplier_lookup_result.read().as_ref() {
                                        p { class: "welcome-back",
                                            "Found supplier: {entry.name}"
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(err) = setup_error.read().as_ref() {
                            p { class: "field-error", "{err}" }
                        }

                        button {
                            disabled: !can_submit,
                            onclick: move |_| {
                                let name = title_case(&name_input.read());
                                let pw = name.to_lowercase();

                                let km = match KeyManager::from_credentials(&name, &pw) {
                                    Ok(km) => km,
                                    Err(e) => {
                                        setup_error.set(Some(format!("{e}")));
                                        return;
                                    }
                                };

                                let postcode = postcode_input.read().trim().to_string();
                                let locality_val = selected_locality.read().clone();
                                let is_sup = *is_supplier.read();
                                let desc = supplier_desc.read().trim().to_string();
                                let lookup_result = supplier_lookup_result.read().clone();

                                {
                                    let mut state = user_state.write();
                                    state.moniker = Some(name.clone());
                                    state.postcode = Some(postcode.clone());
                                    state.locality = locality_val.clone();
                                    if let Some(entry) = lookup_result.as_ref() {
                                        state.is_supplier = false;
                                        state.connected_supplier = Some(entry.name.clone());
                                        state.supplier_node_url = Some(entry.address.clone());
                                        state.supplier_storefront_key = Some(entry.storefront_key.clone());
                                    } else {
                                        state.is_supplier = is_sup;
                                        if is_sup {
                                            state.supplier_description = if desc.is_empty() {
                                                None
                                            } else {
                                                Some(desc.clone())
                                            };
                                        }
                                    }
                                    state.save();
                                }

                                UserState::save_password(&pw);
                                key_manager.set(Some(km));

                                #[cfg(feature = "use-node")]
                                if lookup_result.is_none() && is_sup {
                                    node.send(NodeAction::RegisterSupplier {
                                        name,
                                        postcode,
                                        locality: locality_val,
                                        description: desc,
                                    });
                                }

                                #[cfg(feature = "use-node")]
                                if let Some(sf_key) = lookup_result.as_ref().map(|e| e.storefront_key.clone()) {
                                    node.send(NodeAction::SubscribeCustomerStorefront {
                                        storefront_key: sf_key,
                                    });
                                }
                            },
                            "Get Started"
                        }
                    }
                }
            }
}
