use dioxus::prelude::*;

use cream_common::postcode::format_postcode;

use cream_common::inbox::MessageKind;

use super::app::Route;
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

/// Lists all markets in the system that have upcoming events scheduled.
#[component]
pub fn MarketsListView() -> Element {
    let shared_state = use_shared_state();
    let user_state = use_user_state();
    let nav = use_navigator();
    let node_action = use_node_action();
    let mut prefill_recipient: Signal<Option<String>> = use_context();

    let today = chrono::Utc::now().date_naive();

    struct MarketInfo {
        name: String,
        location: String,
        description: String,
        accepted_count: usize,
        next_event: String,
        organizer_name: Option<String>,
        is_own_market: bool,
    }

    let current_moniker = user_state.read().moniker.clone().unwrap_or_default().to_lowercase();
    let is_supplier = user_state.read().is_supplier;

    let mut markets: Vec<MarketInfo> = Vec::new();
    {
        let shared = shared_state.read();
        for market in shared.market_directory.entries.values() {
            // Only show markets with upcoming events
            let Some(next) = market.next_event(today) else {
                continue;
            };
            let location = format_postcode(
                &market.postcode.clone().unwrap_or_default(),
                market.locality.as_deref(),
            );
            let accepted_count = market.accepted_suppliers().len();
            let next_event = format!(
                "{} ({} – {})",
                next.date.format("%d %b"),
                next.start_time,
                next.end_time
            );
            // Look up the organizer's name from the directory
            let organizer_name = shared
                .directory
                .entries
                .get(&market.organizer)
                .map(|e| e.name.clone());
            let is_own_market = organizer_name
                .as_ref()
                .map(|n| n.to_lowercase() == current_moniker)
                .unwrap_or(false);
            markets.push(MarketInfo {
                name: market.name.clone(),
                location,
                description: market.description.clone(),
                accepted_count,
                next_event,
                organizer_name,
                is_own_market,
            });
        }
    }

    rsx! {
        div { class: "directory-view",
            h2 { "Farmer's Markets" }
            if markets.is_empty() {
                p { class: "empty-state", "No markets with upcoming events." }
            } else {
                div { class: "market-list",
                    {markets.into_iter().map(|m| {
                        let market_name = m.name.clone();
                        let organizer_for_msg = m.organizer_name.clone();
                        rsx! {
                            div { class: "market-card", key: "{m.name}",
                                h3 { "{m.name}" }
                                p { "{m.description}" }
                                p { class: "location", "{m.location}" }
                                p { class: "supplier-count", "{m.accepted_count} suppliers" }
                                p { class: "next-event", "Next: {m.next_event}" }
                                if let Some(ref org) = m.organizer_name {
                                    div { class: "organizer-row",
                                        span { class: "organizer", "Organizer: {org}" }
                                        if !m.is_own_market {
                                            if let Some(ref org) = organizer_for_msg {
                                                {
                                                    let org_for_msg = org.clone();
                                                    let org_for_req = org.clone();
                                                    let market_for_req = market_name.clone();
                                                    rsx! {
                                                        button {
                                                            class: "send-message-btn",
                                                            onclick: move |_| {
                                                                prefill_recipient.set(Some(org_for_msg.clone()));
                                                                nav.push(Route::Messages {});
                                                            },
                                                            "Send Message"
                                                        }
                                                        if is_supplier {
                                                            button {
                                                                class: "request-invite-btn",
                                                                onclick: move |_| {
                                                                    node_action.send(NodeAction::SendInboxMessage {
                                                                        recipient_name: org_for_req.clone(),
                                                                        body: format!("I'd like to participate in '{}'", market_for_req),
                                                                        kind: MessageKind::MarketRequest {
                                                                            market_name: market_for_req.clone(),
                                                                        },
                                                                        recipient_pubkey_hex: None,
                                                                    });
                                                                },
                                                                "Request Invitation"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Link {
                                    to: Route::Market { market_organizer: market_name },
                                    "View Market"
                                }
                            }
                        }
                    })}
                }
            }
        }
    }
}
