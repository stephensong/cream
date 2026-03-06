use dioxus::prelude::*;

use cream_common::inbox::MessageKind;
use cream_common::market::{MarketEvent, SupplierStatus};
use cream_common::postcode::{is_valid_postcode, lookup_all_localities};

use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

/// Organizer dashboard — edit market details, manage events and supplier invites.
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
        // ── Auto-confirm: scan inbox for MarketAccept messages ──
        let market_name = market.name.clone();
        let pending_invites: Vec<String> = market.suppliers.iter()
            .filter(|(_, s)| **s == SupplierStatus::Invited)
            .map(|(name, _)| name.clone())
            .collect();

        // Collect existing supplier names to avoid duplicate invites
        let existing_suppliers: Vec<String> = market.suppliers.keys().cloned().collect();

        if !pending_invites.is_empty() {
            if let Some(inbox) = &shared.inbox {
                for msg in inbox.messages.values() {
                    if let MessageKind::MarketAccept { market_name: ref mn } = msg.kind {
                        if *mn == market_name && pending_invites.contains(&msg.from_name) {
                            // Auto-confirm this supplier
                            node_action.send(NodeAction::ConfirmMarketAcceptance {
                                supplier_name: msg.from_name.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Auto-invite suppliers who sent MarketRequest messages
        if let Some(inbox) = &shared.inbox {
            for msg in inbox.messages.values() {
                if let MessageKind::MarketRequest { market_name: ref mn } = msg.kind {
                    if *mn == market_name && !existing_suppliers.contains(&msg.from_name) {
                        // Auto-invite this supplier and send them a MarketInvite
                        node_action.send(NodeAction::InviteMarketSupplier {
                            supplier_name: msg.from_name.clone(),
                        });
                        node_action.send(NodeAction::SendInboxMessage {
                            recipient_name: msg.from_name.clone(),
                            body: format!("You've been invited to participate in '{}'", market_name),
                            kind: MessageKind::MarketInvite {
                                market_name: market_name.clone(),
                            },
                            recipient_pubkey_hex: None,
                        });
                    }
                }
            }
        }

        let market = market.clone();
        drop(shared);

        let mut editing_events = use_signal(|| false);
        let mut new_event_date = use_signal(String::new);
        let mut new_event_start = use_signal(|| "07:00".to_string());
        let mut new_event_end = use_signal(|| "13:00".to_string());
        let mut invite_name = use_signal(String::new);

        let market_name = market.name.clone();
        let description = market.description.clone();
        let venue = market.venue_address.clone();
        let location_str = cream_common::postcode::format_postcode(
            &market.postcode.clone().unwrap_or_default(),
            market.locality.as_deref(),
        );
        let events = market.events.clone();
        let suppliers: Vec<(String, SupplierStatus)> = market.suppliers.iter()
            .map(|(n, s)| (n.clone(), s.clone()))
            .collect();
        let accepted_count = suppliers.iter().filter(|(_, s)| *s == SupplierStatus::Accepted).count();

        // Next event
        let today = chrono::Utc::now().date_naive();
        let next_event = market.next_event(today);
        let next_event_str = next_event
            .map(|e| format!("{} ({} – {})", e.date.format("%a %d %b %Y"), e.start_time, e.end_time))
            .unwrap_or_else(|| "No upcoming events".to_string());

        // Aggregate order counts from accepted suppliers
        let shared = shared_state.read();
        let mut total_orders = 0usize;
        let mut total_reserved = 0usize;
        for (supplier_name, status) in &suppliers {
            if *status != SupplierStatus::Accepted {
                continue;
            }
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

                div { class: "market-next-event",
                    h3 { "Next Event" }
                    p { class: "next-event-date", "{next_event_str}" }
                }

                div { class: "market-events-section",
                    h3 { "Scheduled Events ({events.len()})" }

                    div { class: "event-list",
                        for event in events.iter() {
                            {
                                let date_str = event.date.format("%a %d %b %Y").to_string();
                                let time_str = format!("{} – {}", event.start_time, event.end_time);
                                let is_past = event.date < today;
                                let class = if is_past { "event-item event-past" } else { "event-item" };
                                rsx! {
                                    div { class: class, key: "{event.date}",
                                        span { class: "event-date", "{date_str}" }
                                        span { class: "event-time", "{time_str}" }
                                        if is_past {
                                            span { class: "event-badge-past", "Past" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if *editing_events.read() {
                        div { class: "add-event-form",
                            label { "Date:" }
                            input {
                                r#type: "date",
                                value: "{new_event_date}",
                                oninput: move |evt| new_event_date.set(evt.value()),
                            }
                            label { "Start:" }
                            input {
                                r#type: "time",
                                value: "{new_event_start}",
                                oninput: move |evt| new_event_start.set(evt.value()),
                            }
                            label { "End:" }
                            input {
                                r#type: "time",
                                value: "{new_event_end}",
                                oninput: move |evt| new_event_end.set(evt.value()),
                            }
                            button {
                                onclick: {
                                    let mut current_events = market.events.clone();
                                    move |_| {
                                        let date_str = new_event_date.read().clone();
                                        if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                                            current_events.push(MarketEvent {
                                                date,
                                                start_time: new_event_start.read().clone(),
                                                end_time: new_event_end.read().clone(),
                                                extra: Default::default(),
                                            });
                                            current_events.sort_by_key(|e| e.date);
                                            node_action.send(NodeAction::UpdateMarketEvents {
                                                events: current_events.clone(),
                                            });
                                            new_event_date.set(String::new());
                                            editing_events.set(false);
                                        }
                                    }
                                },
                                "Add Event"
                            }
                            button {
                                onclick: move |_| editing_events.set(false),
                                "Cancel"
                            }
                        }
                    } else {
                        button {
                            onclick: move |_| editing_events.set(true),
                            "Add Event Date"
                        }
                    }
                }

                div { class: "market-stats",
                    h3 { "Activity" }
                    p { "{total_orders} total orders across {accepted_count} accepted suppliers" }
                    if total_reserved > 0 {
                        p { class: "active-orders", "{total_reserved} active reservations" }
                    }
                }

                div { class: "market-suppliers-section",
                    h3 { "Suppliers ({suppliers.len()})" }

                    div { class: "supplier-list",
                        for (name, status) in suppliers.iter() {
                            {
                                let status_label = match status {
                                    SupplierStatus::Invited => "Invited",
                                    SupplierStatus::Accepted => "Accepted",
                                };
                                let status_class = match status {
                                    SupplierStatus::Invited => "status-invited",
                                    SupplierStatus::Accepted => "status-accepted",
                                };
                                rsx! {
                                    div { class: "supplier-item", key: "{name}",
                                        span { "{name}" }
                                        span { class: "supplier-status {status_class}", "{status_label}" }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "invite-supplier-form",
                        input {
                            r#type: "text",
                            placeholder: "Supplier name to invite...",
                            value: "{invite_name}",
                            oninput: move |evt| invite_name.set(evt.value()),
                        }
                        button {
                            disabled: invite_name.read().trim().is_empty(),
                            onclick: {
                                let market_name_clone = market.name.clone();
                                move |_| {
                                    let name = invite_name.read().trim().to_string();
                                    if !name.is_empty() {
                                        // Update directory (add as Invited)
                                        node_action.send(NodeAction::InviteMarketSupplier {
                                            supplier_name: name.clone(),
                                        });
                                        // Send inbox message
                                        node_action.send(NodeAction::SendInboxMessage {
                                            recipient_name: name,
                                            body: format!("You've been invited to participate in '{}'", market_name_clone),
                                            kind: MessageKind::MarketInvite {
                                                market_name: market_name_clone.clone(),
                                            },
                                            recipient_pubkey_hex: None,
                                        });
                                        invite_name.set(String::new());
                                    }
                                }
                            },
                            "Invite Supplier"
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
        let mut submitted = use_signal(|| false);

        let postcode_val = postcode.read().clone();
        let localities = if is_valid_postcode(&postcode_val) {
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
                            timezone: cream_common::postcode::timezone_for_postcode(&pc),
                        });
                        submitted.set(true);
                    },
                    "Register Market"
                }
            }
        }
    }
}
