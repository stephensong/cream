use dioxus::prelude::*;

use cream_common::postcode::format_postcode;

use super::app::Route;
use super::shared_state::use_shared_state;

/// Lists all markets in the system that have upcoming events scheduled.
#[component]
pub fn MarketsListView() -> Element {
    let shared_state = use_shared_state();
    let nav = use_navigator();
    let mut prefill_recipient: Signal<Option<String>> = use_context();

    let today = chrono::Utc::now().date_naive();

    struct MarketInfo {
        name: String,
        location: String,
        description: String,
        accepted_count: usize,
        next_event: String,
        organizer_name: Option<String>,
    }

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
            markets.push(MarketInfo {
                name: market.name.clone(),
                location,
                description: market.description.clone(),
                accepted_count,
                next_event,
                organizer_name,
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
                                        if let Some(org) = organizer_for_msg {
                                            button {
                                                class: "send-message-btn",
                                                onclick: move |_| {
                                                    prefill_recipient.set(Some(org.clone()));
                                                    nav.push(Route::Messages {});
                                                },
                                                "Send Message"
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
