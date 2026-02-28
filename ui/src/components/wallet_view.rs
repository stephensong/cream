use dioxus::prelude::*;

use cream_common::currency::format_amount;
use cream_common::identity::ROOT_USER_NAME;
use cream_common::lightning_gateway::CURD_PER_SAT;
use cream_common::wallet::TransactionKind;

use super::lightning_remote::LightningClient;
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

/// State for a pending peg-in operation.
#[derive(Clone, Debug, PartialEq)]
enum PegInState {
    /// No peg-in in progress.
    Idle,
    /// Invoice created, waiting for user to pay.
    WaitingForPayment {
        bolt11: String,
        payment_hash: String,
        amount_sats: u64,
    },
    /// HTLC accepted, allocating CURD.
    Accepted {
        payment_hash: String,
        amount_sats: u64,
    },
    /// CURD allocated and invoice settled.
    Complete,
    /// Something went wrong.
    Error(String),
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

    let has_gateway = LightningClient::is_available();

    // Peg-in state
    let mut pegin_sats = use_signal(|| String::new());
    let mut pegin_state = use_signal(|| PegInState::Idle);
    // Peg-out state
    let mut pegout_curd = use_signal(|| String::new());
    let mut pegout_bolt11 = use_signal(|| String::new());

    // Read signals eagerly so Dioxus subscribes to changes for button disabled state
    let pegin_sats_val: u64 = pegin_sats.read().parse().unwrap_or(0);
    let pegout_curd_val: u64 = pegout_curd.read().parse().unwrap_or(0);
    let pegout_bolt11_empty = pegout_bolt11.read().is_empty();
    let current_pegin_state = pegin_state.read().clone();

    // Polling coroutine for pending peg-in
    let _pegin_poll = use_coroutine(move |_rx: UnboundedReceiver<()>| {
        async move {
            loop {
                // Check if we need to poll
                let state = pegin_state.read().clone();
                match state {
                    PegInState::WaitingForPayment { ref payment_hash, amount_sats, .. } => {
                        let hash = payment_hash.clone();
                        if let Some(client) = LightningClient::from_env() {
                            match client.check_pegin_status(&hash).await {
                                Ok(resp) if resp.status == "accepted" => {
                                    pegin_state.set(PegInState::Accepted {
                                        payment_hash: hash.clone(),
                                        amount_sats,
                                    });
                                    // Allocate CURD
                                    let node = use_node_action();
                                    node.send(NodeAction::PegInAllocate {
                                        amount_sats,
                                        payment_hash: hash,
                                    });
                                    pegin_state.set(PegInState::Complete);
                                    // Auto-clear after a few seconds
                                    #[cfg(target_family = "wasm")]
                                    gloo_timers::future::TimeoutFuture::new(3_000).await;
                                    pegin_state.set(PegInState::Idle);
                                }
                                Ok(resp) if resp.status == "settled" => {
                                    pegin_state.set(PegInState::Complete);
                                    #[cfg(target_family = "wasm")]
                                    gloo_timers::future::TimeoutFuture::new(3_000).await;
                                    pegin_state.set(PegInState::Idle);
                                }
                                Ok(resp) if resp.status == "cancelled" => {
                                    pegin_state.set(PegInState::Error("Invoice cancelled".to_string()));
                                }
                                Ok(_) => {
                                    // Still waiting — poll again
                                }
                                Err(e) => {
                                    pegin_state.set(PegInState::Error(e));
                                }
                            }
                        }
                    }
                    _ => {}
                }

                // Poll every 3 seconds
                #[cfg(target_family = "wasm")]
                gloo_timers::future::TimeoutFuture::new(3_000).await;
                #[cfg(not(target_family = "wasm"))]
                std::future::pending::<()>().await; // never runs on native
            }
        }
    });

    rsx! {
        div { class: "wallet-view",
            h2 { "CREAM Wallet" }
            div { class: "balance-display",
                h3 { class: "wallet-balance", "Balance: {balance_str}" }
            }
            if is_supplier && incoming_deposits > 0 {
                p { class: "wallet-deposits", "Includes {deposits_str} held in escrow" }
            }

            p { class: "exchange-rate", "Exchange rate: 1 sat = {CURD_PER_SAT} CURD" }

            // ── Peg-In ──
            div { class: "peg-section",
                h3 { "Deposit (Lightning Peg-In)" }

                // Show pending peg-in state
                match &current_pegin_state {
                    PegInState::WaitingForPayment { bolt11, amount_sats, .. } => rsx! {
                        div { class: "pegin-pending",
                            p { class: "pegin-status", "Waiting for payment..." }
                            p { "Pay this invoice ({amount_sats} sats) with your Lightning wallet:" }
                            div { class: "bolt11-display",
                                code { class: "bolt11-string", "{bolt11}" }
                                button {
                                    class: "copy-btn",
                                    onclick: {
                                        let _bolt11 = bolt11.clone();
                                        move |_| {
                                            #[cfg(target_family = "wasm")]
                                            {
                                                if let Some(window) = web_sys::window() {
                                                    if let Some(clipboard) = window.navigator().clipboard() {
                                                        let _ = clipboard.write_text(&_bolt11);
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    "Copy"
                                }
                            }
                            button {
                                class: "cancel-btn",
                                onclick: move |_| {
                                    let state = pegin_state.read().clone();
                                    if let PegInState::WaitingForPayment { ref payment_hash, .. } = state {
                                        let hash = payment_hash.clone();
                                        spawn(async move {
                                            if let Some(client) = LightningClient::from_env() {
                                                let _ = client.cancel_pegin(&hash).await;
                                            }
                                            pegin_state.set(PegInState::Idle);
                                        });
                                    }
                                },
                                "Cancel"
                            }
                        }
                    },
                    PegInState::Accepted { amount_sats, .. } => rsx! {
                        div { class: "pegin-pending",
                            p { class: "pegin-status pegin-accepted", "Payment received! Allocating {amount_sats * CURD_PER_SAT} CURD..." }
                        }
                    },
                    PegInState::Complete => rsx! {
                        div { class: "pegin-pending",
                            p { class: "pegin-status pegin-complete", "Deposit complete!" }
                        }
                    },
                    PegInState::Error(msg) => rsx! {
                        div { class: "pegin-pending pegin-error",
                            p { class: "pegin-status", "Error: {msg}" }
                            button {
                                onclick: move |_| pegin_state.set(PegInState::Idle),
                                "Dismiss"
                            }
                        }
                    },
                    PegInState::Idle => rsx! {
                        div { class: "form-group",
                            label { "Amount (sats)" }
                            input {
                                r#type: "number",
                                min: "1",
                                placeholder: "e.g. 100",
                                value: "{pegin_sats}",
                                oninput: move |e| pegin_sats.set(e.value()),
                            }
                            if pegin_sats_val > 0 {
                                {
                                    let curd = pegin_sats_val * CURD_PER_SAT;
                                    rsx! { p { class: "peg-preview", "You will receive {format_amount(curd)}" } }
                                }
                            }
                        }
                        if has_gateway {
                            // Real Lightning peg-in
                            button {
                                disabled: pegin_sats_val == 0,
                                onclick: move |_| {
                                    let sats: u64 = pegin_sats.read().parse().unwrap_or(0);
                                    if sats > 0 {
                                        spawn(async move {
                                            if let Some(client) = LightningClient::from_env() {
                                                match client.create_pegin_invoice(sats).await {
                                                    Ok(resp) => {
                                                        pegin_state.set(PegInState::WaitingForPayment {
                                                            bolt11: resp.bolt11,
                                                            payment_hash: resp.payment_hash,
                                                            amount_sats: resp.amount_sats,
                                                        });
                                                        pegin_sats.set(String::new());
                                                    }
                                                    Err(e) => {
                                                        pegin_state.set(PegInState::Error(e));
                                                    }
                                                }
                                            }
                                        });
                                    }
                                },
                                "Generate Lightning Invoice"
                            }
                        } else {
                            // Mock/dev peg-in (instant)
                            button {
                                disabled: pegin_sats_val == 0,
                                onclick: move |_| {
                                    let sats: u64 = pegin_sats.read().parse().unwrap_or(0);
                                    if sats > 0 {
                                        let node = use_node_action();
                                        node.send(NodeAction::PegIn { amount_sats: sats });
                                        pegin_sats.set(String::new());
                                    }
                                },
                                "Deposit via Lightning"
                            }
                        }
                    },
                }
            }

            // ── Peg-Out ──
            div { class: "peg-section",
                h3 { "Withdraw (Lightning Peg-Out)" }
                div { class: "form-group",
                    label { "Amount (CURD)" }
                    input {
                        r#type: "number",
                        min: "1",
                        placeholder: "e.g. 500",
                        value: "{pegout_curd}",
                        oninput: move |e| pegout_curd.set(e.value()),
                    }
                    if pegout_curd_val > 0 {
                        {
                            let sats = pegout_curd_val / CURD_PER_SAT;
                            rsx! { p { class: "peg-preview", "You will withdraw {sats} sats" } }
                        }
                    }
                }
                div { class: "form-group",
                    label { "BOLT11 Invoice" }
                    input {
                        r#type: "text",
                        placeholder: "lnbc...",
                        value: "{pegout_bolt11}",
                        oninput: move |e| pegout_bolt11.set(e.value()),
                    }
                }
                button {
                    disabled: pegout_curd_val == 0 || pegout_bolt11_empty,
                    onclick: move |_| {
                        let curd: u64 = pegout_curd.read().parse().unwrap_or(0);
                        let bolt11 = pegout_bolt11.read().clone();
                        if curd > 0 && !bolt11.is_empty() {
                            let node = use_node_action();
                            if has_gateway {
                                node.send(NodeAction::PegOutViaGateway { amount_curd: curd, bolt11 });
                            } else {
                                node.send(NodeAction::PegOut { amount_curd: curd, bolt11 });
                            }
                            pegout_curd.set(String::new());
                            pegout_bolt11.set(String::new());
                        }
                    },
                    "Withdraw to Lightning"
                }
            }

            // ── Faucet (dev only) ──
            div { class: "wallet-actions",
                button {
                    onclick: move |_| {
                        let node = use_node_action();
                        node.send(NodeAction::FaucetTopUp);
                    },
                    "Faucet (+1000 CURD)"
                }
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
