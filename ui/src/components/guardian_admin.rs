//! Guardian operator dashboard for Lightning gateway management.
//!
//! Provides a `/guardian` route showing LND node status, channel management,
//! and peg-in/peg-out history. Only accessible when `CREAM_GATEWAY_URL` is set.

use dioxus::prelude::*;

use super::lightning_remote::{
    BalanceResponse, ChannelInfo, LightningClient, LndInfo, PegTransaction,
};

#[component]
pub fn GuardianAdmin() -> Element {
    let mut lnd_info = use_signal(|| None::<LndInfo>);
    let mut balance = use_signal(|| None::<BalanceResponse>);
    let mut channels = use_signal(|| Vec::<ChannelInfo>::new());
    let mut history = use_signal(|| Vec::<PegTransaction>::new());
    let mut error_msg = use_signal(|| None::<String>);
    let mut loading = use_signal(|| true);

    // Channel management state
    let mut open_pubkey = use_signal(String::new);
    let mut open_amount = use_signal(String::new);

    // Initial data fetch
    use_effect(move || {
        spawn(async move {
            if let Some(client) = LightningClient::from_env() {
                let mut had_error = false;

                match client.get_info().await {
                    Ok(info) => lnd_info.set(Some(info)),
                    Err(e) => {
                        error_msg.set(Some(format!("LND info: {}", e)));
                        had_error = true;
                    }
                }

                if !had_error {
                    if let Ok(bal) = client.get_balance().await {
                        balance.set(Some(bal));
                    }
                    if let Ok(chs) = client.list_channels().await {
                        channels.set(chs);
                    }
                    if let Ok(hist) = client.get_history().await {
                        history.set(hist);
                    }
                }
            } else {
                error_msg.set(Some("No Lightning gateway configured".to_string()));
            }
            loading.set(false);
        });
    });

    let is_loading = *loading.read();

    rsx! {
        div { class: "guardian-admin",
            h2 { "Guardian Admin" }

            if is_loading {
                p { "Loading LND status..." }
            }

            if let Some(ref err) = *error_msg.read() {
                div { class: "alert alert-error", "{err}" }
            }

            // ── LND Node Status ──
            if let Some(ref info) = *lnd_info.read() {
                div { class: "card",
                    h3 { "LND Node" }
                    table {
                        tbody {
                            tr { td { "Pubkey" } td { class: "mono", "{info.pubkey}" } }
                            tr { td { "Alias" } td { "{info.alias}" } }
                            tr { td { "Block Height" } td { "{info.block_height}" } }
                            tr {
                                td { "Synced" }
                                td {
                                    if info.synced_to_chain { "Chain " } else { "" }
                                    if info.synced_to_graph { "Graph" } else { "" }
                                }
                            }
                            tr { td { "Active Channels" } td { "{info.num_active_channels}" } }
                            tr { td { "Peers" } td { "{info.num_peers}" } }
                        }
                    }
                }
            }

            // ── Balance Summary ──
            if let Some(ref bal) = *balance.read() {
                div { class: "card",
                    h3 { "Balance" }
                    div { class: "balance-grid",
                        div { class: "balance-item",
                            span { class: "balance-label", "On-chain" }
                            span { class: "balance-value", "{bal.wallet.confirmed_balance} sats" }
                            if bal.wallet.unconfirmed_balance > 0 {
                                span { class: "balance-pending", "(+{bal.wallet.unconfirmed_balance} pending)" }
                            }
                        }
                        div { class: "balance-item",
                            span { class: "balance-label", "Outbound (local)" }
                            span { class: "balance-value", "{bal.channel.local_balance_sat} sats" }
                        }
                        div { class: "balance-item",
                            span { class: "balance-label", "Inbound (remote)" }
                            span { class: "balance-value", "{bal.channel.remote_balance_sat} sats" }
                        }
                    }

                    // Liquidity alerts
                    {
                        let local = bal.channel.local_balance_sat;
                        let remote = bal.channel.remote_balance_sat;
                        let total = local + remote;
                        if total > 0 {
                            let outbound_pct = (local as f64 / total as f64 * 100.0) as u64;
                            let inbound_pct = 100 - outbound_pct;
                            rsx! {
                                div { class: "capacity-bar",
                                    div {
                                        class: "capacity-local",
                                        style: "width: {outbound_pct}%",
                                        "Out {outbound_pct}%"
                                    }
                                    div {
                                        class: "capacity-remote",
                                        style: "width: {inbound_pct}%",
                                        "In {inbound_pct}%"
                                    }
                                }
                                if local < 10_000 {
                                    p { class: "alert alert-warning", "Low outbound capacity! Peg-outs may fail." }
                                }
                                if remote < 10_000 {
                                    p { class: "alert alert-warning", "Low inbound capacity! Peg-ins may fail." }
                                }
                            }
                        } else {
                            rsx! {}
                        }
                    }
                }
            }

            // ── Channels ──
            div { class: "card",
                h3 { "Channels" }
                if channels.read().is_empty() {
                    p { "No channels" }
                } else {
                    table { class: "channel-table",
                        thead {
                            tr {
                                th { "Peer" }
                                th { "Capacity" }
                                th { "Local" }
                                th { "Remote" }
                                th { "Status" }
                                th { "Actions" }
                            }
                        }
                        tbody {
                            for ch in channels.read().iter() {
                                {
                                    let peer_short = if ch.remote_pubkey.len() > 12 {
                                        format!("{}...", &ch.remote_pubkey[..12])
                                    } else {
                                        ch.remote_pubkey.clone()
                                    };
                                    let cp = ch.channel_point.clone();
                                    rsx! {
                                        tr {
                                            td { class: "mono", "{peer_short}" }
                                            td { "{ch.capacity}" }
                                            td { "{ch.local_balance}" }
                                            td { "{ch.remote_balance}" }
                                            td { if ch.active { "Active" } else { "Inactive" } }
                                            td {
                                                button {
                                                    class: "btn-sm btn-danger",
                                                    onclick: move |_| {
                                                        let cp = cp.clone();
                                                        spawn(async move {
                                                            if let Some(client) = LightningClient::from_env() {
                                                                match client.close_channel(&cp, false).await {
                                                                    Ok(()) => {
                                                                        // Refresh channels
                                                                        if let Ok(chs) = client.list_channels().await {
                                                                            channels.set(chs);
                                                                        }
                                                                    }
                                                                    Err(e) => error_msg.set(Some(format!("Close failed: {}", e))),
                                                                }
                                                            }
                                                        });
                                                    },
                                                    "Close"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Open channel form
                h4 { "Open Channel" }
                div { class: "form-row",
                    input {
                        r#type: "text",
                        placeholder: "Node pubkey (hex)",
                        value: "{open_pubkey}",
                        oninput: move |e| open_pubkey.set(e.value()),
                    }
                    input {
                        r#type: "number",
                        placeholder: "Amount (sats)",
                        value: "{open_amount}",
                        oninput: move |e| open_amount.set(e.value()),
                    }
                    button {
                        disabled: open_pubkey.read().is_empty() || open_amount.read().is_empty(),
                        onclick: move |_| {
                            let pubkey = open_pubkey.read().clone();
                            let amount: i64 = open_amount.read().parse().unwrap_or(0);
                            if amount > 0 {
                                spawn(async move {
                                    if let Some(client) = LightningClient::from_env() {
                                        match client.open_channel(&pubkey, amount).await {
                                            Ok(cp) => {
                                                error_msg.set(Some(format!("Channel opened: {}", cp)));
                                                open_pubkey.set(String::new());
                                                open_amount.set(String::new());
                                                if let Ok(chs) = client.list_channels().await {
                                                    channels.set(chs);
                                                }
                                            }
                                            Err(e) => error_msg.set(Some(format!("Open failed: {}", e))),
                                        }
                                    }
                                });
                            }
                        },
                        "Open Channel"
                    }
                }
            }

            // ── Peg History ──
            div { class: "card",
                h3 { "Peg History" }
                if history.read().is_empty() {
                    p { "No transactions yet" }
                } else {
                    table { class: "peg-history",
                        thead {
                            tr {
                                th { "Type" }
                                th { "Amount (sats)" }
                                th { "Status" }
                                th { "Payment Hash" }
                                th { "Time" }
                            }
                        }
                        tbody {
                            for tx in history.read().iter().rev().take(50) {
                                {
                                    let hash_short = if tx.payment_hash.len() > 16 {
                                        format!("{}...", &tx.payment_hash[..16])
                                    } else {
                                        tx.payment_hash.clone()
                                    };
                                    rsx! {
                                        tr {
                                            td { class: if tx.kind == "peg-in" { "peg-in-label" } else { "peg-out-label" },
                                                "{tx.kind}"
                                            }
                                            td { "{tx.amount_sats}" }
                                            td { "{tx.status}" }
                                            td { class: "mono", "{hash_short}" }
                                            td { "{tx.timestamp}" }
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
