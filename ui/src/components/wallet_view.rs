use dioxus::prelude::*;

use cream_common::currency::{format_amount, Currency};

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
    let currency = state.currency.clone();
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
    let balance_str = format_amount(displayed_balance, &currency);
    let deposits_str = format_amount(incoming_deposits, &currency);

    rsx! {
        div { class: "wallet-view",
            h2 { "CREAM Wallet" }
            div { class: "balance-display",
                h3 { class: "wallet-balance", "Balance: {balance_str}" }
            }
            if is_supplier && incoming_deposits > 0 {
                p { class: "wallet-deposits", "Includes {deposits_str} from order deposits" }
            }

            div { class: "form-group",
                label { "Display currency:" }
                select {
                    value: "{currency.label()}",
                    onchange: move |evt: Event<FormData>| {
                        let new_currency = match evt.value().as_str() {
                            "Sats" => Currency::Sats,
                            "AUD" => Currency::Cents,
                            _ => Currency::Curds,
                        };
                        let mut state = user_state.write();
                        state.currency = new_currency;
                        state.save();
                    },
                    {Currency::all().iter().map(|c| {
                        let label = c.label();
                        let desc = match c {
                            Currency::Curds => "Curds (CURD)",
                            Currency::Sats => "Sats (Bitcoin)",
                            Currency::Cents => "AUD (illustrative)",
                        };
                        rsx! {
                            option { value: "{label}", "{desc}" }
                        }
                    })}
                }
            }
            if currency == Currency::Cents {
                p { class: "wallet-note", "* AUD amounts are illustrative (placeholder rate: 1 BTC ≈ $150k AUD)." }
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
