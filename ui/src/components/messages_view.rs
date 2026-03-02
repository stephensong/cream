use dioxus::prelude::*;

use cream_common::inbox::{InboxMessage, MessageKind, INBOX_MESSAGE_COST_CURD};

use super::chat_client::{
    ChatMessage, ChatSession, ChatState, ChatWsHandle, ClientMsg, SessionStatus,
};
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

#[component]
pub fn MessagesView() -> Element {
    let shared_state = use_shared_state();
    let user_state = use_user_state();
    let mut chat = use_context::<Signal<ChatState>>();
    let ws_handle = use_context::<Signal<ChatWsHandle>>();
    let mut compose_to = use_signal(String::new);
    let mut compose_body = use_signal(String::new);
    let mut send_error = use_signal(|| None::<String>);

    let balance = shared_state
        .read()
        .user_contract
        .as_ref()
        .map(|uc| uc.balance_curds)
        .unwrap_or(0);
    let cost = INBOX_MESSAGE_COST_CURD;
    let my_name = user_state.read().moniker.clone().unwrap_or_default();

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

    // Get directory names for the compose recipient picker
    let directory_names: Vec<String> = {
        let shared = shared_state.read();
        shared
            .directory
            .entries
            .values()
            .map(|e| e.name.clone())
            .filter(|n| *n != my_name)
            .collect()
    };

    let connected = chat.read().connected;

    // Find recipient's public key (needed for chat requests)
    let recipient_pubkey: Option<String> = {
        let to = compose_to.read().clone();
        if to.is_empty() {
            None
        } else {
            let shared_read = shared_state.read();
            shared_read
                .directory
                .entries
                .values()
                .find(|e| e.name == to)
                .map(|e| {
                    let bytes = e.supplier.0.to_bytes();
                    bytes
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<String>()
                })
        }
    };

    let send_disabled = compose_to.read().is_empty()
        || compose_body.read().trim().is_empty()
        || balance < cost;
    let chat_disabled =
        send_disabled || recipient_pubkey.is_none() || !connected;

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
                        for name in directory_names.iter() {
                            option { value: "{name}", "{name}" }
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
                        onclick: move |_| {
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
                            let node = use_node_action();
                            node.send(NodeAction::SendInboxMessage {
                                recipient_name: to,
                                body,
                                kind: MessageKind::DirectMessage,
                            });
                            compose_to.set(String::new());
                            compose_body.set(String::new());
                        },
                        "Send Message"
                    }
                    button {
                        class: "request-chat-btn",
                        disabled: chat_disabled,
                        onclick: {
                            let my_name = my_name.clone();
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
                                let node = use_node_action();

                                if let Some(ref pubkey) = recipient_pubkey {
                                    node.send(NodeAction::ChatMessageToll);

                                    let session_id = format!(
                                        "chat-{}",
                                        chrono::Utc::now().timestamp_millis()
                                    );
                                    node.send(NodeAction::SendInboxMessage {
                                        recipient_name: to.clone(),
                                        body: body.clone(),
                                        kind: MessageKind::ChatInvite {
                                            session_id: session_id.clone(),
                                        },
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
                                        has_av: false,
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

/// Action button for chat invites in the inbox: opens the chat panel if the
/// sender is online, or shows "Sender offline" otherwise.
#[component]
fn ChatInviteAction(session_id: String, peer_name: String) -> Element {
    let mut chat = use_context::<Signal<ChatState>>();
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
    } else if is_connected {
        rsx! {
            span { class: "messages-item-offline", "Chat session ended" }
        }
    } else {
        rsx! {
            span { class: "messages-item-offline", "Chat relay offline" }
        }
    }
}
