use std::collections::BTreeSet;

use dioxus::prelude::*;

use cream_common::postcode::{is_valid_au_postcode, lookup_all_localities};
use cream_common::storefront::WeeklySchedule;

use super::node_api::{use_node_action, NodeAction};
use super::schedule_editor::ScheduleSummary;
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

/// Organizer dashboard — edit market details, manage supplier list.
#[component]
pub fn MarketDashboard() -> Element {
    let shared_state = use_shared_state();
    let _user_state = use_user_state();
    let node_action = use_node_action();

    let km_signal: Signal<Option<crate::components::key_manager::KeyManager>> = use_context();
    let my_id = {
        let km = km_signal.read();
        km.as_ref().map(|km| km.user_id())
    };

    let shared = shared_state.read();
    let existing_market = my_id.as_ref().and_then(|id| shared.market_directory.entries.get(id));

    if let Some(market) = existing_market {
        // ── Existing market: show details + management ──
        let market = market.clone();
        drop(shared);

        let mut editing_suppliers = use_signal(|| false);
        let mut new_supplier_name = use_signal(String::new);

        let market_name = market.name.clone();
        let description = market.description.clone();
        let venue = market.venue_address.clone();
        let location_str = cream_common::postcode::format_postcode(
            &market.postcode.clone().unwrap_or_default(),
            market.locality.as_deref(),
        );
        let schedule = market.schedule.clone();
        let _timezone = market.timezone.clone().unwrap_or_default();
        let suppliers: Vec<String> = market.suppliers.iter().cloned().collect();

        // Aggregate order counts from participating suppliers
        let shared = shared_state.read();
        let mut total_orders = 0usize;
        let mut total_reserved = 0usize;
        for supplier_name in &suppliers {
            if let Some(sf) = shared.storefronts.get(supplier_name) {
                total_orders += sf.orders.len();
                total_reserved += sf.orders.values()
                    .filter(|o| matches!(o.status, cream_common::order::OrderStatus::Reserved { .. }))
                    .count();
            }
        }
        drop(shared);

        rsx! {
            div { class: "market-dashboard",
                h2 { "Market: {market_name}" }
                p { class: "market-description", "{description}" }
                p { class: "market-venue", "Venue: {venue}" }
                p { class: "market-location", "{location_str}" }

                div { class: "market-schedule-section",
                    h3 { "Opening Hours" }
                    ScheduleSummary { schedule: schedule }
                }

                div { class: "market-stats",
                    h3 { "Activity" }
                    p { "{total_orders} total orders across {suppliers.len()} suppliers" }
                    if total_reserved > 0 {
                        p { class: "active-orders", "{total_reserved} active reservations" }
                    }
                }

                div { class: "market-suppliers-section",
                    h3 { "Participating Suppliers ({suppliers.len()})" }

                    div { class: "supplier-list",
                        {suppliers.iter().map(|name| {
                            let name_clone = name.clone();
                            let suppliers_set = market.suppliers.clone();
                            rsx! {
                                div { class: "supplier-item", key: "{name}",
                                    span { "{name}" }
                                    if *editing_suppliers.read() {
                                        button {
                                            class: "remove-btn",
                                            onclick: move |_| {
                                                let mut updated: BTreeSet<String> = suppliers_set.clone();
                                                updated.remove(&name_clone);
                                                node_action.send(NodeAction::UpdateMarketSuppliers {
                                                    suppliers: updated,
                                                });
                                                editing_suppliers.set(false);
                                            },
                                            "Remove"
                                        }
                                    }
                                }
                            }
                        })}
                    }

                    if *editing_suppliers.read() {
                        div { class: "add-supplier-form",
                            input {
                                r#type: "text",
                                placeholder: "Supplier name...",
                                value: "{new_supplier_name}",
                                oninput: move |evt| new_supplier_name.set(evt.value()),
                            }
                            button {
                                onclick: {
                                    let suppliers_set = market.suppliers.clone();
                                    move |_| {
                                        let name = new_supplier_name.read().trim().to_string();
                                        if !name.is_empty() {
                                            let mut updated = suppliers_set.clone();
                                            updated.insert(name);
                                            node_action.send(NodeAction::UpdateMarketSuppliers {
                                                suppliers: updated,
                                            });
                                            new_supplier_name.set(String::new());
                                            editing_suppliers.set(false);
                                        }
                                    }
                                },
                                "Add Supplier"
                            }
                            button {
                                onclick: move |_| editing_suppliers.set(false),
                                "Cancel"
                            }
                        }
                    } else {
                        button {
                            onclick: move |_| editing_suppliers.set(true),
                            "Edit Suppliers"
                        }
                    }
                }
            }
        }
    } else {
        // ── No market yet: show registration form ──
        drop(shared);

        let mut name = use_signal(String::new);
        let mut description = use_signal(String::new);
        let mut venue_address = use_signal(String::new);
        let mut postcode = use_signal(String::new);
        let mut locality = use_signal(|| None::<String>);
        let mut supplier_input = use_signal(String::new);
        let mut suppliers: Signal<BTreeSet<String>> = use_signal(BTreeSet::new);
        let mut submitted = use_signal(|| false);

        let postcode_val = postcode.read().clone();
        let localities = if is_valid_au_postcode(&postcode_val) {
            lookup_all_localities(&postcode_val)
        } else {
            vec![]
        };

        if *submitted.read() {
            return rsx! {
                div { class: "market-dashboard",
                    h2 { "Market Registered!" }
                    p { "Your market has been submitted to the network." }
                }
            };
        }

        rsx! {
            div { class: "market-dashboard",
                h2 { "Register a New Market" }
                p { "Set up a farmer's market where multiple suppliers can sell their products." }

                div { class: "form-group",
                    label { "Market Name:" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. Coffs Harbour Farmers Market",
                        value: "{name}",
                        oninput: move |evt| name.set(evt.value()),
                    }
                }

                div { class: "form-group",
                    label { "Description:" }
                    textarea {
                        placeholder: "Describe your market...",
                        value: "{description}",
                        oninput: move |evt| description.set(evt.value()),
                    }
                }

                div { class: "form-group",
                    label { "Venue Address:" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. Coffs Harbour Showground, Stadium Dr",
                        value: "{venue_address}",
                        oninput: move |evt| venue_address.set(evt.value()),
                    }
                }

                div { class: "form-group",
                    label { "Postcode:" }
                    input {
                        r#type: "text",
                        placeholder: "e.g. 2450",
                        value: "{postcode}",
                        oninput: move |evt| postcode.set(evt.value()),
                    }
                }

                if !localities.is_empty() {
                    div { class: "form-group",
                        label { "Locality:" }
                        select {
                            onchange: move |evt| {
                                let v = evt.value();
                                locality.set(if v.is_empty() { None } else { Some(v) });
                            },
                            option { value: "", "Select locality..." }
                            {localities.iter().map(|loc| {
                                let name = &loc.place_name;
                                rsx! { option { value: "{name}", "{name}" } }
                            })}
                        }
                    }
                }

                div { class: "form-group",
                    label { "Participating Suppliers:" }
                    div { class: "supplier-chips",
                        {suppliers.read().iter().map(|s| {
                            let s_clone = s.clone();
                            rsx! {
                                span { class: "supplier-chip", key: "{s}",
                                    "{s}"
                                    button {
                                        class: "chip-remove",
                                        onclick: move |_| {
                                            suppliers.write().remove(&s_clone);
                                        },
                                        "x"
                                    }
                                }
                            }
                        })}
                    }
                    div { class: "add-supplier-inline",
                        input {
                            r#type: "text",
                            placeholder: "Supplier name...",
                            value: "{supplier_input}",
                            oninput: move |evt| supplier_input.set(evt.value()),
                        }
                        button {
                            onclick: move |_| {
                                let n = supplier_input.read().trim().to_string();
                                if !n.is_empty() {
                                    suppliers.write().insert(n);
                                    supplier_input.set(String::new());
                                }
                            },
                            "Add"
                        }
                    }
                }

                button {
                    disabled: name.read().trim().is_empty() || venue_address.read().trim().is_empty(),
                    onclick: move |_| {
                        let pc = postcode.read().clone();
                        node_action.send(NodeAction::RegisterMarket {
                            name: name.read().trim().to_string(),
                            description: description.read().trim().to_string(),
                            venue_address: venue_address.read().trim().to_string(),
                            postcode: pc.clone(),
                            locality: locality.read().clone(),
                            schedule: WeeklySchedule::default(),
                            timezone: cream_common::postcode::timezone_for_postcode(&pc)
                                .map(|s| s.to_string()),
                            suppliers: suppliers.read().clone(),
                        });
                        submitted.set(true);
                    },
                    "Register Market"
                }
            }
        }
    }
}
