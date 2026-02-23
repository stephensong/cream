use dioxus::prelude::*;

use cream_common::currency::format_amount;

use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::{use_user_state, TransactionKind};

#[component]
pub fn WalletView() -> Element {
    let mut user_state = use_user_state();
    let shared_state = use_shared_state();

    let state = user_state.read();
    let base_balance = state.balance();
    let moniker = state.moniker.clone().unwrap_or_default();
    let is_supplier = state.is_supplier;
    let recent_txs: Vec<_> = state.ledger.iter().rev().take(20).cloned().collect();
    drop(state);

    // Compute incoming deposit credits from network orders on this supplier's storefront
    let incoming_deposits: u64 = if is_supplier {
        let shared = shared_state.read();
        shared
            .storefronts
            .get(&moniker)
            .map(|sf| {
                sf.orders
                    .values()
                    .filter(|o| {
                        matches!(
                            o.status,
                            cream_common::order::OrderStatus::Reserved { .. }
                                | cream_common::order::OrderStatus::Paid
                        )
                    })
                    .map(|o| o.deposit_amount)
                    .sum()
            })
            .unwrap_or(0)
    } else {
        0
    };

    let displayed_balance = base_balance + incoming_deposits;
    let balance_str = format_amount(displayed_balance);
    let deposits_str = format_amount(incoming_deposits);

    rsx! {
        div { class: "wallet-view",
            h2 { "CREAM Wallet" }
            div { class: "balance-display",
                h3 { class: "wallet-balance", "Balance: {balance_str}" }
            }
            if is_supplier && incoming_deposits > 0 {
                p { class: "wallet-deposits", "Includes {deposits_str} from order deposits" }
            }

            div { class: "wallet-actions",
                button {
                    onclick: move |_| {
                        let new_balance = {
                            let mut state = user_state.write();
                            state.record_credit(1000, "Faucet".into());
                            state.save();
                            state.balance()
                        };
                        // Sync to user contract on the network
                        let node = use_node_action();
                        node.send(NodeAction::UpdateUserContract {
                            current_supplier: None,
                            balance_curds: Some(new_balance),
                        });
                    },
                    "Faucet (+1000 CURD)"
                }
            }
            p { class: "wallet-note",
                "This is a mock wallet for demonstration. Fedimint integration coming later."
            }

            if !recent_txs.is_empty() {
                h3 { "Recent Transactions" }
                table { class: "tx-history",
                    thead {
                        tr {
                            th { "Time" }
                            th { "Description" }
                            th { "Amount" }
                        }
                    }
                    tbody {
                        for tx in &recent_txs {
                            tr {
                                td { class: "tx-time", "{short_timestamp(&tx.timestamp)}" }
                                td { "{tx.description}" }
                                td { class: match tx.kind {
                                        TransactionKind::Credit => "tx-credit",
                                        TransactionKind::Debit => "tx-debit",
                                    },
                                    {match tx.kind {
                                        TransactionKind::Credit => format!("+{}", tx.amount),
                                        TransactionKind::Debit => format!("-{}", tx.amount),
                                    }}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Format an ISO 8601 timestamp to a short display form.
fn short_timestamp(ts: &str) -> String {
    // "2026-02-23T10:30:00.000Z" → "Feb 23, 10:30"
    if ts.len() >= 16 {
        let month = match &ts[5..7] {
            "01" => "Jan", "02" => "Feb", "03" => "Mar", "04" => "Apr",
            "05" => "May", "06" => "Jun", "07" => "Jul", "08" => "Aug",
            "09" => "Sep", "10" => "Oct", "11" => "Nov", "12" => "Dec",
            _ => "???",
        };
        let day = &ts[8..10];
        let time = &ts[11..16];
        format!("{} {}, {}", month, day.trim_start_matches('0'), time)
    } else {
        ts.to_string()
    }
}
