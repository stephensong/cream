//! Guardian admin dashboard: toll rate management + Lightning gateway management.
//!
//! Accessible to admin users (determined by guardian `--admin-pubkeys`).
//! Toll rate editor is always shown. Lightning sections only appear when
//! `CREAM_GATEWAY_URL` is configured.

use dioxus::prelude::*;

use cream_common::tolls::TollRates;

use super::key_manager::KeyManager;
use super::lightning_remote::{
    BalanceResponse, ChannelInfo, LightningClient, LndInfo, PegTransaction, ReconciliationReport,
};
use super::node_api::{use_node_action, NodeAction};
use super::toll_rates::AdminStatus;

#[component]
pub fn GuardianAdmin() -> Element {
    // ── Toll Rate Editor state ──
    let toll_rates: Signal<TollRates> = use_context();

    let mut session_toll = use_signal(String::new);
    let mut session_interval = use_signal(String::new);
    let mut inbox_message = use_signal(String::new);
    let mut curd_per_sat = use_signal(String::new);

    // Sync input fields when toll_rates context updates (initial fetch + periodic poll)
    use_effect(move || {
        let current = toll_rates.read().clone();
        session_toll.set(current.session_toll_curd.to_string());
        session_interval.set(current.session_interval_secs.to_string());
        inbox_message.set(current.inbox_message_curd.to_string());
        curd_per_sat.set(current.curd_per_sat.to_string());
    });
    let mut toll_feedback = use_signal(|| None::<String>);
    let mut toll_error = use_signal(|| None::<String>);
    let node_action = use_node_action();

    // ── Admin management state (root only) ──
    let admin_status: Signal<AdminStatus> = use_context();
    let is_root = admin_status.read().root;
    let mut admin_list = use_signal(|| Vec::<String>::new());
    let mut admin_input = use_signal(String::new);
    let mut admin_feedback = use_signal(|| None::<String>);
    let mut admin_error = use_signal(|| None::<String>);

    // Fetch admin list on mount if root
    {
        let is_root = is_root;
        use_effect(move || {
            if is_root {
                spawn(async move {
                    let km: Signal<Option<KeyManager>> = use_context();
                    let pubkey_hex = {
                        match km.read().as_ref() {
                            Some(km) => km.pubkey_hex(),
                            None => return,
                        }
                    };
                    match super::toll_rates::fetch_admin_list(&pubkey_hex).await {
                        Ok(list) => admin_list.set(list),
                        Err(e) => admin_error.set(Some(format!("Failed to load admins: {}", e))),
                    }
                });
            }
        });
    }

    // ── Lightning state ──
    let mut lnd_info = use_signal(|| None::<LndInfo>);
    let mut balance = use_signal(|| None::<BalanceResponse>);
    let mut channels = use_signal(|| Vec::<ChannelInfo>::new());
    let mut history = use_signal(|| Vec::<PegTransaction>::new());
    let mut error_msg = use_signal(|| None::<String>);
    let mut loading = use_signal(|| true);

    // Channel management state
    let mut open_pubkey = use_signal(String::new);
    let mut open_amount = use_signal(String::new);

    // Reconciliation state
    let mut reconciliation = use_signal(|| None::<ReconciliationReport>);
    let mut recon_error = use_signal(|| None::<String>);

    let has_lightning = LightningClient::is_available();

    // Initial Lightning data fetch
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
                    match client.get_reconciliation().await {
                        Ok(report) => reconciliation.set(Some(report)),
                        Err(e) => recon_error.set(Some(e)),
                    }
                }
            }
            loading.set(false);
        });
    });

    let is_loading = *loading.read();

    rsx! {
        div { class: "guardian-admin",
            h2 { "Guardian Admin" }

            // ── Toll Rate Editor (root only) ──
            if is_root {
                div { class: "card",
                    h3 { "Toll Rates" }
                    if let Some(ref msg) = *toll_feedback.read() {
                        div { class: "alert alert-success", "{msg}" }
                    }
                    if let Some(ref err) = *toll_error.read() {
                        div { class: "alert alert-error", "{err}" }
                    }
                    div { class: "form-grid",
                        label { "Session Toll (CURD)" }
                        input {
                            r#type: "number",
                            min: "0",
                            value: "{session_toll}",
                            oninput: move |e| session_toll.set(e.value()),
                        }
                        label { "Session Interval (secs)" }
                        input {
                            r#type: "number",
                            min: "1",
                            value: "{session_interval}",
                            oninput: move |e| session_interval.set(e.value()),
                        }
                        label { "Inbox Message (CURD)" }
                        input {
                            r#type: "number",
                            min: "0",
                            value: "{inbox_message}",
                            oninput: move |e| inbox_message.set(e.value()),
                        }
                        label { "CURD per Sat" }
                        input {
                            r#type: "number",
                            min: "1",
                            value: "{curd_per_sat}",
                            oninput: move |e| curd_per_sat.set(e.value()),
                        }
                    }
                    button {
                        class: "btn-primary",
                        onclick: move |_| {
                            let new_rates = TollRates {
                                session_toll_curd: session_toll.read().parse().unwrap_or(1),
                                session_interval_secs: session_interval.read().parse().unwrap_or(10),
                                inbox_message_curd: inbox_message.read().parse().unwrap_or(1),
                                curd_per_sat: curd_per_sat.read().parse().unwrap_or(10),
                                extra: Default::default(),
                            };
                            node_action.send(NodeAction::SetTollRates { rates: new_rates });
                            toll_feedback.set(Some("Toll rates saved".to_string()));
                            toll_error.set(None);
                        },
                        "Save Toll Rates"
                    }
                }
            }

            // ── Admin Management (root only) ──
            if is_root {
                div { class: "card",
                    h3 { "Manage Admins" }
                    if let Some(ref msg) = *admin_feedback.read() {
                        div { class: "alert alert-success", "{msg}" }
                    }
                    if let Some(ref err) = *admin_error.read() {
                        div { class: "alert alert-error", "{err}" }
                    }

                    // Current admin list
                    if admin_list.read().is_empty() {
                        p { "No admins loaded" }
                    } else {
                        table {
                            thead {
                                tr {
                                    th { "Pubkey" }
                                    th { "Role" }
                                    th { "Actions" }
                                }
                            }
                            tbody {
                                for (i, admin_pk) in admin_list.read().iter().enumerate() {
                                    {
                                        let pk = admin_pk.clone();
                                        let pk_short = if pk.len() > 16 {
                                            format!("{}...{}", &pk[..8], &pk[pk.len()-8..])
                                        } else {
                                            pk.clone()
                                        };
                                        let is_self_root = i == 0;
                                        rsx! {
                                            tr {
                                                td { class: "mono", "{pk_short}" }
                                                td { if is_self_root { "Root" } else { "Admin" } }
                                                td {
                                                    if !is_self_root {
                                                        button {
                                                            class: "btn-sm btn-danger",
                                                            onclick: move |_| {
                                                                let pk = pk.clone();
                                                                spawn(async move {
                                                                    let km: Signal<Option<KeyManager>> = use_context();
                                                                    let grantor = match km.read().as_ref() {
                                                                        Some(km) => km.pubkey_hex(),
                                                                        None => return,
                                                                    };
                                                                    match super::toll_rates::revoke_admin(&pk, &grantor).await {
                                                                        Ok(list) => {
                                                                            admin_list.set(list);
                                                                            admin_feedback.set(Some("Admin revoked".to_string()));
                                                                            admin_error.set(None);
                                                                        }
                                                                        Err(e) => {
                                                                            admin_error.set(Some(format!("Revoke failed: {}", e)));
                                                                            admin_feedback.set(None);
                                                                        }
                                                                    }
                                                                });
                                                            },
                                                            "Revoke"
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

                    // Grant admin form
                    h4 { "Grant Admin" }
                    div { class: "form-row",
                        input {
                            r#type: "text",
                            placeholder: "User pubkey (hex)",
                            value: "{admin_input}",
                            oninput: move |e| admin_input.set(e.value()),
                        }
                        button {
                            disabled: admin_input.read().is_empty(),
                            onclick: move |_| {
                                let pk = admin_input.read().clone();
                                spawn(async move {
                                    let km: Signal<Option<KeyManager>> = use_context();
                                    let grantor = match km.read().as_ref() {
                                        Some(km) => km.pubkey_hex(),
                                        None => return,
                                    };
                                    match super::toll_rates::grant_admin(&pk, &grantor).await {
                                        Ok(list) => {
                                            admin_list.set(list);
                                            admin_input.set(String::new());
                                            admin_feedback.set(Some("Admin granted".to_string()));
                                            admin_error.set(None);
                                        }
                                        Err(e) => {
                                            admin_error.set(Some(format!("Grant failed: {}", e)));
                                            admin_feedback.set(None);
                                        }
                                    }
                                });
                            },
                            "Grant Admin"
                        }
                    }
                }
            }

            // ── Lightning sections (only if gateway configured) ──
            if has_lightning {
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

                // ── Reconciliation ──
                div { class: "card",
                    h3 { "Reconciliation" }
                    if let Some(ref err) = *recon_error.read() {
                        div { class: "alert alert-warning", "{err}" }
                    }
                    if let Some(ref report) = *reconciliation.read() {
                        table {
                            tbody {
                                tr { td { "Total CURD in circulation" } td { "{report.total_curd_in_circulation}" } }
                                tr { td { "Total sats backing (local channel)" } td { "{report.total_sats_backing}" } }
                                tr { td { "Expected sats (CURD / rate)" } td { "{report.expected_sats}" } }
                                tr { td { "CURD per sat" } td { "{report.curd_per_sat}" } }
                                tr {
                                    td { "Discrepancy" }
                                    td {
                                        class: if report.discrepancy_sats.abs() > 0 { "alert-text" } else { "" },
                                        "{report.discrepancy_sats} sats"
                                    }
                                }
                                tr { td { "User contracts checked" } td { "{report.user_count}" } }
                                tr { td { "Checked at" } td { "{report.checked_at}" } }
                            }
                        }
                        if !report.warnings.is_empty() {
                            div { class: "alert alert-warning",
                                h4 { "Warnings" }
                                for w in report.warnings.iter() {
                                    p { "{w}" }
                                }
                            }
                        }
                        button {
                            class: "btn-secondary",
                            onclick: move |_| {
                                spawn(async move {
                                    if let Some(client) = LightningClient::from_env() {
                                        match client.get_reconciliation().await {
                                            Ok(r) => {
                                                reconciliation.set(Some(r));
                                                recon_error.set(None);
                                            }
                                            Err(e) => recon_error.set(Some(e)),
                                        }
                                    }
                                });
                            },
                            "Refresh"
                        }
                    } else if recon_error.read().is_none() {
                        p { "Loading..." }
                    }
                }
            }
        }
    }
}
