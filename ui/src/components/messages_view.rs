use std::collections::BTreeMap;

use dioxus::prelude::*;

use cream_common::inbox::{InboxMessage, MessageKind};

use super::chat_client::{
    ChatMessage, ChatSession, ChatState, ChatWsHandle, ClientMsg, SessionStatus,
};
use super::chat_view::use_peer_presence;
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

/// A recipient entry: display name + pubkey hex.
/// `from_directory` is true for suppliers in the directory (no need to pass pubkey to SendInboxMessage).
#[derive(Clone, Debug)]
struct RecipientEntry {
    /// Name shown in the dropdown (may include " (admin)" suffix).
    display_name: String,
    /// Name sent to the handler for directory lookup (without suffix).
    recipient_name: String,
    pubkey_hex: String,
    from_directory: bool,
}

#[component]
pub fn MessagesView() -> Element {
    let shared_state = use_shared_state();
    let user_state = use_user_state();
    let mut chat = use_context::<Signal<ChatState>>();
    let ws_handle = use_context::<Signal<ChatWsHandle>>();
    let node_action = use_node_action();
    let mut prefill_recipient: Signal<Option<String>> = use_context();
    let mut compose_to = use_signal(String::new);
    let mut compose_body = use_signal(String::new);
    let mut send_error = use_signal(|| None::<String>);
    let mut admin_pubkeys = use_signal(Vec::<String>::new);

    let balance = shared_state
        .read()
        .user_contract
        .as_ref()
        .map(|uc| uc.balance_curds)
        .unwrap_or(0);
    let toll_rates = super::toll_rates::use_toll_rates();
    let cost = toll_rates.read().inbox_message_curd;
    let my_name = user_state.read().moniker.clone().unwrap_or_default();

    // Fetch admin list once on mount
    let _admin_fetch = use_resource(move || {
        async move {
            let km_signal: Signal<Option<crate::components::key_manager::KeyManager>> =
                use_context();
            let pubkey_hex = {
                let km = km_signal.read();
                match km.as_ref() {
                    Some(km) => km.pubkey_hex(),
                    None => return,
                }
            };
            match super::toll_rates::fetch_admin_list(&pubkey_hex).await {
                Ok(list) => admin_pubkeys.set(list),
                Err(_) => {} // Silently ignore — non-admins can't fetch admin list
            }
        }
    });

    // Combine received inbox messages and locally-tracked sent messages
    let messages: Vec<(InboxMessage, Option<String>)> = {
        let shared = shared_state.read();
        // Received messages: to_name = None (they're addressed to us)
        let mut all: Vec<(InboxMessage, Option<String>)> = shared
            .inbox
            .as_ref()
            .map(|inbox| {
                inbox
                    .messages
                    .values()
                    .cloned()
                    .map(|m| (m, None))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // Sent messages: to_name = Some(recipient)
        for sent in &shared.sent_messages {
            all.push((sent.message.clone(), Some(sent.to_name.clone())));
        }
        all.sort_by(|a, b| b.0.created_at.cmp(&a.0.created_at));
        all
    };

    // Build recipient list: directory suppliers + admin/root entries
    let recipients: Vec<RecipientEntry> = {
        let shared = shared_state.read();
        let admin_list = admin_pubkeys.read();
        let admin_set: std::collections::HashSet<&str> =
            admin_list.iter().map(|s| s.as_str()).collect();
        let mut by_name: BTreeMap<String, RecipientEntry> = BTreeMap::new();

        // Add directory suppliers, tagging admins
        for entry in shared.directory.entries.values() {
            if entry.name == my_name {
                continue;
            }
            let pubkey_hex = entry
                .supplier
                .0
                .to_bytes()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            let is_admin = admin_set.contains(pubkey_hex.as_str());
            let display_name = if is_admin {
                format!("{} (admin)", entry.name)
            } else {
                entry.name.clone()
            };
            by_name.insert(
                entry.name.clone(),
                RecipientEntry {
                    display_name,
                    recipient_name: entry.name.clone(),
                    pubkey_hex,
                    from_directory: true,
                },
            );
        }

        // Add root
        {
            let root_id = cream_common::identity::root_user_id();
            let root_hex = root_id
                .0
                .to_bytes()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            let my_km: Signal<Option<crate::components::key_manager::KeyManager>> = use_context();
            let is_me_root = my_km
                .read()
                .as_ref()
                .map(|km| km.pubkey_hex() == root_hex)
                .unwrap_or(false);
            if !is_me_root {
                by_name
                    .entry("Root".to_string())
                    .or_insert(RecipientEntry {
                        display_name: "Root".to_string(),
                        recipient_name: "Root".to_string(),
                        pubkey_hex: root_hex,
                        from_directory: false,
                    });
            }
        }

        // Add non-directory admins from the fetched admin list
        let directory_pubkeys: std::collections::HashSet<String> =
            by_name.values().map(|r| r.pubkey_hex.clone()).collect();
        for (i, pk) in admin_list.iter().enumerate() {
            if i == 0 {
                continue; // Root already added above
            }
            if directory_pubkeys.contains(pk) {
                continue; // Already in list (supplier who is also admin)
            }
            let short = if pk.len() > 16 {
                format!("Admin ({}...{})", &pk[..6], &pk[pk.len() - 6..])
            } else {
                format!("Admin ({})", pk)
            };
            by_name.entry(short.clone()).or_insert(RecipientEntry {
                display_name: short.clone(),
                recipient_name: short,
                pubkey_hex: pk.clone(),
                from_directory: false,
            });
        }

        by_name.into_values().collect()
    };

    // Apply prefill from context (e.g. "Send Message" from markets view).
    // Build the display name map so the effect can resolve it.
    let recipient_display_map: std::collections::HashMap<String, String> = recipients.iter()
        .map(|r| (r.recipient_name.clone(), r.display_name.clone()))
        .collect();
    use_effect(move || {
        let prefill = prefill_recipient.read().clone();
        if let Some(name) = prefill {
            let display = recipient_display_map.get(&name).cloned().unwrap_or(name);
            compose_to.set(display);
            prefill_recipient.set(None);
        }
    });

    let connected = chat.read().connected;

    // Find recipient's public key (needed for chat requests and non-directory sends)
    let selected_recipient: Option<RecipientEntry> = {
        let to = compose_to.read().clone();
        if to.is_empty() {
            None
        } else {
            recipients.iter().find(|r| r.display_name == to).cloned()
        }
    };
    let recipient_pubkey: Option<String> = selected_recipient.as_ref().map(|r| r.pubkey_hex.clone());

    let peer_online = use_peer_presence(&recipient_pubkey);

    let send_disabled = compose_to.read().is_empty()
        || compose_body.read().trim().is_empty()
        || balance < cost;
    let chat_disabled =
        send_disabled || recipient_pubkey.is_none() || !connected || !peer_online;

    rsx! {
        div { class: "messages-view",
            h2 { "Messages" }

            // Compose form
            div { class: "messages-compose",
                h3 { "New Message" }
                div { class: "form-group",
                    label { "To:" }
                    select {
                        value: "{compose_to}",
                        onchange: move |evt| compose_to.set(evt.value()),
                        option { value: "", "Select recipient..." }
                        for r in recipients.iter() {
                            option { value: "{r.display_name}", "{r.display_name}" }
                        }
                    }
                }
                div { class: "form-group",
                    textarea {
                        placeholder: "Write a message...",
                        maxlength: "1000",
                        value: "{compose_body}",
                        oninput: move |evt| {
                            compose_body.set(evt.value());
                            send_error.set(None);
                        },
                    }
                }
                div { class: "messages-compose-footer",
                    if let Some(err) = send_error.read().as_ref() {
                        span { class: "field-error", "{err}" }
                    }
                    button {
                        disabled: send_disabled,
                        onclick: {
                            let selected = selected_recipient.clone();
                            move |_| {
                                let to = compose_to.read().clone();
                                let body = compose_body.read().trim().to_string();
                                if to.is_empty() || body.is_empty() {
                                    return;
                                }
                                if balance < cost {
                                    send_error.set(Some(format!(
                                        "Insufficient balance (need {} CURD)",
                                        cost
                                    )));
                                    return;
                                }
                                let (name, pubkey_hex) = match selected.as_ref() {
                                    Some(r) => (
                                        r.recipient_name.clone(),
                                        if r.from_directory { None } else { Some(r.pubkey_hex.clone()) },
                                    ),
                                    None => (to, None),
                                };
                                node_action.send(NodeAction::SendInboxMessage {
                                    recipient_name: name,
                                    body,
                                    kind: MessageKind::DirectMessage,
                                    recipient_pubkey_hex: pubkey_hex,
                                });
                                compose_to.set(String::new());
                                compose_body.set(String::new());
                            }
                        },
                        "Send Message"
                    }
                    button {
                        class: "request-chat-btn",
                        disabled: chat_disabled,
                        onclick: {
                            let my_name = my_name.clone();
                            let selected = selected_recipient.clone();
                            move |_| {
                                let to = compose_to.read().clone();
                                let body = compose_body.read().trim().to_string();
                                if to.is_empty() || body.is_empty() {
                                    return;
                                }
                                if balance < cost {
                                    send_error.set(Some(format!(
                                        "Insufficient balance (need {} CURD)",
                                        cost
                                    )));
                                    return;
                                }
                                if let Some(ref pubkey) = recipient_pubkey {
                                    let session_id = format!(
                                        "chat-{}",
                                        chrono::Utc::now().timestamp_millis()
                                    );
                                    let (name, pk_hex) = match selected.as_ref() {
                                        Some(r) => (
                                            r.recipient_name.clone(),
                                            if r.from_directory { None } else { Some(r.pubkey_hex.clone()) },
                                        ),
                                        None => (to.clone(), None),
                                    };
                                    node_action.send(NodeAction::SendInboxMessage {
                                        recipient_name: name,
                                        body: body.clone(),
                                        kind: MessageKind::ChatInvite {
                                            session_id: session_id.clone(),
                                        },
                                        recipient_pubkey_hex: pk_hex,
                                    });

                                    let session = ChatSession {
                                        session_id: session_id.clone(),
                                        peer_pubkey: pubkey.clone(),
                                        peer_name: to.clone(),
                                        messages: vec![ChatMessage {
                                            sender_is_me: true,
                                            sender_name: my_name.clone(),
                                            body: body.clone(),
                                            timestamp: chrono::Utc::now(),
                                        }],
                                        started_at: chrono::Utc::now(),
                                        status: SessionStatus::PendingAccept,
                                        mic_enabled: false,
                                        speaker_enabled: false,
                                        camera_enabled: false,
                                        tv_enabled: false,
                                        has_remote_video: false,
                                        is_initiator: true,
                                        payment_request: None,
                                    };
                                    {
                                        let mut state = chat.write();
                                        state
                                            .sessions
                                            .insert(session_id.clone(), session);
                                        state.panel_open = true;
                                    }

                                    ws_handle.read().send(&ClientMsg::Invite {
                                        to: pubkey.clone(),
                                        session_id,
                                        ecdh_pubkey: String::new(),
                                        message: body,
                                    });
                                }
                                compose_to.set(String::new());
                                compose_body.set(String::new());
                            }
                        },
                        "Request Chat"
                    }
                }
            }

            // Message list
            if messages.is_empty() {
                p { class: "empty-state", "No messages yet." }
            } else {
                div { class: "messages-list",
                    for (msg, to_name) in messages.iter() {
                        {
                            let is_sent = to_name.is_some();
                            let kind_badge = match (&msg.kind, is_sent) {
                                (_, true) => "Sent",
                                (MessageKind::DirectMessage, false) => "DM",
                                (MessageKind::ChatInvite { .. }, false) => "Chat Invite",
                                (MessageKind::MarketInvite { .. }, false) => "Market Invite",
                                (MessageKind::MarketAccept { .. }, false) => "Market Accept",
                            };
                            let peer_label = if let Some(to) = to_name {
                                format!("To: {to}")
                            } else {
                                msg.from_name.clone()
                            };
                            let time_str = msg.created_at.format("%d %b %H:%M").to_string();
                            let body_preview = if msg.body.len() > 200 {
                                format!("{}...", &msg.body[..197])
                            } else {
                                msg.body.clone()
                            };
                            let item_class = if is_sent {
                                "messages-item messages-item-sent"
                            } else {
                                "messages-item"
                            };
                            rsx! {
                                div { class: item_class, key: "{msg.id}",
                                    div { class: "messages-item-header",
                                        span { class: "messages-item-sender", "{peer_label}" }
                                        span { class: "messages-item-badge", "{kind_badge}" }
                                        span { class: "messages-item-time", "{time_str}" }
                                    }
                                    p { class: "messages-item-body", "{body_preview}" }
                                    if !is_sent {
                                        if let MessageKind::ChatInvite { session_id } = &msg.kind {
                                            {
                                                let session_id = session_id.clone();
                                                let peer_name = msg.from_name.clone();
                                                rsx! {
                                                    ChatInviteAction {
                                                        session_id: session_id,
                                                        peer_name: peer_name,
                                                    }
                                                }
                                            }
                                        }
                                        if let MessageKind::MarketInvite { market_name } = &msg.kind {
                                            {
                                                let market_name = market_name.clone();
                                                let organizer_name = msg.from_name.clone();
                                                rsx! {
                                                    MarketInviteAction {
                                                        market_name: market_name,
                                                        organizer_name: organizer_name,
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
        }
    }
}

/// Action button for chat invites in the inbox: opens the chat panel if a
/// session exists, offers to join if the relay is connected, or shows offline.
#[component]
fn ChatInviteAction(session_id: String, peer_name: String) -> Element {
    let mut chat = use_context::<Signal<ChatState>>();
    let ws_handle = use_context::<Signal<ChatWsHandle>>();
    let shared_state = use_shared_state();

    // Resolve peer pubkey for presence check
    let peer_pubkey: Option<String> = {
        let shared = shared_state.read();
        shared.directory.entries.values()
            .find(|e| e.name == peer_name)
            .map(|e| {
                let bytes = e.supplier.0.to_bytes();
                bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>()
            })
    };
    let peer_online = use_peer_presence(&peer_pubkey);

    let chat_read = chat.read();
    let has_session = chat_read.sessions.contains_key(&session_id);
    let is_connected = chat_read.connected;
    drop(chat_read);

    if has_session {
        rsx! {
            button {
                class: "chat-start-btn",
                onclick: move |_| {
                    chat.write().panel_open = true;
                },
                "Open Chat"
            }
        }
    } else if is_connected && peer_online {
        // No relay session exists (missed the ephemeral WebSocket invite).
        // Let the user join by sending a fresh relay invite with the same session_id.
        rsx! {
            button {
                class: "chat-start-btn",
                onclick: {
                    let session_id = session_id.clone();
                    let peer_name = peer_name.clone();
                    let peer_pubkey = peer_pubkey.clone();
                    move |_| {
                        if let Some(pubkey) = peer_pubkey.clone() {
                            let session = ChatSession {
                                session_id: session_id.clone(),
                                peer_pubkey: pubkey.clone(),
                                peer_name: peer_name.clone(),
                                messages: vec![],
                                started_at: chrono::Utc::now(),
                                status: SessionStatus::PendingAccept,
                                mic_enabled: false,
                                speaker_enabled: false,
                                camera_enabled: false,
                                tv_enabled: false,
                                has_remote_video: false,
                                is_initiator: true,
                                payment_request: None,
                            };
                            {
                                let mut state = chat.write();
                                state.sessions.insert(session_id.clone(), session);
                                state.panel_open = true;
                            }

                            ws_handle.read().send(&ClientMsg::Invite {
                                to: pubkey,
                                session_id: session_id.clone(),
                                ecdh_pubkey: String::new(),
                                message: String::new(),
                            });
                        }
                    }
                },
                "Join Chat"
            }
        }
    } else if is_connected {
        rsx! {
            span { class: "messages-item-offline", "Peer offline" }
        }
    } else {
        rsx! {
            span { class: "messages-item-offline", "Chat relay offline" }
        }
    }
}

/// Action buttons for market invite messages: Accept or Decline.
#[component]
fn MarketInviteAction(market_name: String, organizer_name: String) -> Element {
    let node_action = use_node_action();
    let shared_state = use_shared_state();
    let user_state = use_user_state();

    // Check if we've already accepted (our name appears as Accepted in the market)
    let my_name = user_state.read().moniker.clone().unwrap_or_default();
    let already_accepted = {
        let shared = shared_state.read();
        shared.market_directory.entries.values()
            .find(|m| m.name == market_name)
            .and_then(|m| m.suppliers.get(&my_name))
            .map(|s| *s == cream_common::market::SupplierStatus::Accepted)
            .unwrap_or(false)
    };

    if already_accepted {
        rsx! {
            span { class: "market-invite-accepted", "Accepted" }
        }
    } else {
        rsx! {
            div { class: "market-invite-actions",
                button {
                    class: "accept-btn",
                    onclick: {
                        let market_name = market_name.clone();
                        let organizer_name = organizer_name.clone();
                        move |_| {
                            // Send MarketAccept inbox to organizer
                            node_action.send(NodeAction::SendInboxMessage {
                                recipient_name: organizer_name.clone(),
                                body: format!("I accept the invitation to '{}'", market_name),
                                kind: MessageKind::MarketAccept {
                                    market_name: market_name.clone(),
                                },
                                recipient_pubkey_hex: None,
                            });
                        }
                    },
                    "Accept"
                }
                span { class: "market-invite-pending", "Pending" }
            }
        }
    }
}
