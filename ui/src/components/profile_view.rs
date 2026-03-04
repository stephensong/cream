use dioxus::prelude::*;

use super::key_manager::KeyManager;
use super::shared_state::use_shared_state;
use super::toll_rates::AdminStatus;
use super::user_state::use_user_state;
use cream_common::postcode::format_postcode;

/// Copy text to clipboard via the navigator.clipboard API.
#[cfg(target_family = "wasm")]
pub async fn copy_to_clipboard(text: &str) {
    if let Some(window) = web_sys::window() {
        let clipboard = window.navigator().clipboard();
        let promise = clipboard.write_text(text);
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
}

#[component]
pub fn ProfileView() -> Element {
    let user_state = use_user_state();
    let key_manager: Signal<Option<KeyManager>> = use_context();
    let shared = use_shared_state();
    let admin_status = *use_context::<Signal<AdminStatus>>().read();

    let state = user_state.read();
    let moniker = state.moniker.clone().unwrap_or_default();
    let postcode_raw = state.postcode.clone().unwrap_or_default();
    let locality = state.locality.clone();
    let postcode_display = format_postcode(&postcode_raw, locality.as_deref());
    let is_supplier = state.is_supplier;
    let supplier_description = state.supplier_description.clone();
    let is_customer = state.connected_supplier.is_some();
    drop(state);

    let pubkey_hex = key_manager
        .read()
        .as_ref()
        .map(|km| km.pubkey_hex())
        .unwrap_or_default();

    let shared_read = shared.read();
    let balance = shared_read
        .user_contract
        .as_ref()
        .map(|uc| uc.balance_curds)
        .unwrap_or(0);

    let has_products = shared_read
        .storefronts
        .get(&moniker)
        .map(|sf| !sf.products.is_empty())
        .unwrap_or(false);
    let role_label = if has_products {
        "Supplier"
    } else if is_customer {
        "Guest"
    } else {
        "User"
    };
    drop(shared_read);

    #[allow(unused_mut)]
    let mut copied = use_signal(|| false);

    rsx! {
        section { class: "profile-page",
            h2 { "My Profile" }

            div { class: "profile-section",
                h3 { "Identity" }
                div { class: "profile-pubkey-row",
                    span { class: "mono profile-pubkey", "{pubkey_hex}" }
                    button {
                        class: "copy-btn",
                        style: "margin-left: 0.75em;",
                        onclick: move |_| {
                            #[cfg(target_family = "wasm")]
                            {
                                let pk = pubkey_hex.clone();
                                spawn(async move {
                                    copy_to_clipboard(&pk).await;
                                    copied.set(true);
                                    gloo_timers::future::TimeoutFuture::new(2000).await;
                                    copied.set(false);
                                });
                            }
                        },
                        if *copied.read() { "Copied!" } else { "Copy" }
                    }
                }
            }

            div { class: "profile-section",
                h3 { "Name & Location" }
                p { "Moniker: {moniker}" }
                p { "Location: {postcode_display}" }
            }

            div { class: "profile-section",
                h3 { "Role" }
                span { class: "role-badge", "{role_label}" }
                if admin_status.root {
                    span { class: "admin-badge root-admin", " Root Admin" }
                } else if admin_status.admin {
                    span { class: "admin-badge", " Admin" }
                }
            }

            if is_supplier {
                if let Some(desc) = supplier_description.as_deref() {
                    if !desc.is_empty() {
                        div { class: "profile-section",
                            h3 { "Supplier Bio" }
                            p { "{desc}" }
                        }
                    }
                }
            }

            div { class: "profile-section",
                h3 { "CURD Balance" }
                p { class: "balance-display", "{balance} CURD" }
            }
        }
    }
}
