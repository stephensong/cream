use dioxus::prelude::*;

use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

#[component]
pub fn WalletView() -> Element {
    let mut user_state = use_user_state();
    let shared_state = use_shared_state();

    let state = user_state.read();
    let base_balance = state.balance;
    let moniker = state.moniker.clone().unwrap_or_default();
    let is_supplier = state.is_supplier;
    drop(state);

    // Compute incoming deposit credits from network orders on this supplier's storefront
    let incoming_deposits: u64 = if is_supplier {
        let shared = shared_state.read();
        shared
            .storefronts
            .get(&moniker)
            .map(|sf| sf.orders.values().map(|o| o.deposit_amount).sum())
            .unwrap_or(0)
    } else {
        0
    };

    let displayed_balance = base_balance + incoming_deposits;

    rsx! {
        div { class: "wallet-view",
            h2 { "CURD Wallet" }
            div { class: "balance-display",
                h3 { class: "wallet-balance", "Balance: {displayed_balance} CURD" }
            }
            if is_supplier && incoming_deposits > 0 {
                p { class: "wallet-deposits", "Includes {incoming_deposits} CURD from order deposits" }
            }
            div { class: "wallet-actions",
                button {
                    onclick: move |_| {
                        let mut state = user_state.write();
                        state.balance += 1000;
                        state.save();
                    },
                    "Faucet (+1000 CURD)"
                }
            }
            p { class: "wallet-note",
                "This is a mock wallet for demonstration. Fedimint integration coming later."
            }
        }
    }
}
