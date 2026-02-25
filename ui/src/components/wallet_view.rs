use dioxus::prelude::*;

use cream_common::currency::format_amount;
use cream_common::identity::ROOT_USER_NAME;
use cream_common::wallet::TransactionKind;

use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

/// Map internal names to display names.
fn display_name(name: &str) -> &str {
    if name == ROOT_USER_NAME {
        "System"
    } else {
        name
    }
}

#[component]
pub fn WalletView() -> Element {
    let user_state = use_user_state();
    let shared_state = use_shared_state();

    let moniker = user_state.read().moniker.clone().unwrap_or_default();
    let is_supplier = user_state.read().is_supplier;

    // Read balance and transactions from the on-network user contract
    let shared = shared_state.read();
    let (base_balance, recent_txs) = if let Some(ref uc) = shared.user_contract {
        let txs: Vec<_> = uc.ledger.iter().rev().take(20).cloned().collect();
        (uc.balance_curds, txs)
    } else {
        (0, Vec::new())
    };
    drop(shared);

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
                        let node = use_node_action();
                        node.send(NodeAction::FaucetTopUp);
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
                            th { "Counterparty" }
                            th { "Amount" }
                        }
                    }
                    tbody {
                        for tx in &recent_txs {
                            {
                                let counterparty = match tx.kind {
                                    TransactionKind::Credit => display_name(&tx.sender),
                                    TransactionKind::Debit => display_name(&tx.receiver),
                                };
                                rsx! {
                                    tr {
                                        td { class: "tx-time", "{short_timestamp(&tx.timestamp)}" }
                                        td { "{tx.description}" }
                                        td { "{counterparty}" }
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
