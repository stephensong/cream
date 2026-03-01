use dioxus::prelude::*;

use cream_common::inbox::{InboxMessage, MessageKind, INBOX_MESSAGE_COST_CURD};

use super::chat_client::ChatState;
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

#[component]
pub fn MessagesView() -> Element {
    let shared_state = use_shared_state();
    let user_state = use_user_state();
    let mut compose_to = use_signal(String::new);
    let mut compose_body = use_signal(String::new);
    let mut send_error = use_signal(|| None::<String>);

    let balance = shared_state
        .read()
        .user_contract
        .as_ref()
        .map(|uc| uc.balance_curds)
        .unwrap_or(0);

    // Get inbox messages sorted newest first
    let messages: Vec<InboxMessage> = {
        let shared = shared_state.read();
        shared
            .inbox
            .as_ref()
            .map(|inbox| {
                let mut msgs: Vec<_> = inbox.messages.values().cloned().collect();
                msgs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                msgs
            })
            .unwrap_or_default()
    };

    // Get directory names for the compose recipient picker
    let directory_names: Vec<String> = {
        let shared = shared_state.read();
        let my_name = user_state.read().moniker.clone().unwrap_or_default();
        shared
            .directory
            .entries
            .values()
            .map(|e| e.name.clone())
            .filter(|n| *n != my_name)
            .collect()
    };

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
                    span { class: "toll-badge", "Cost: {INBOX_MESSAGE_COST_CURD} CURD" }
                    if let Some(err) = send_error.read().as_ref() {
                        span { class: "field-error", "{err}" }
                    }
                    button {
                        disabled: compose_to.read().is_empty()
                            || compose_body.read().trim().is_empty()
                            || balance < INBOX_MESSAGE_COST_CURD,
                        onclick: move |_| {
                            let to = compose_to.read().clone();
                            let body = compose_body.read().trim().to_string();
                            if to.is_empty() || body.is_empty() {
                                return;
                            }
                            if balance < INBOX_MESSAGE_COST_CURD {
                                send_error.set(Some(format!(
                                    "Insufficient balance (need {} CURD)",
                                    INBOX_MESSAGE_COST_CURD
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
                        "Send ({INBOX_MESSAGE_COST_CURD} CURD)"
                    }
                }
            }

            // Message list
            if messages.is_empty() {
                p { class: "empty-state", "No messages yet." }
            } else {
                div { class: "messages-list",
                    for msg in messages.iter() {
                        {
                            let kind_badge = match &msg.kind {
                                MessageKind::DirectMessage => "DM",
                                MessageKind::ChatInvite { .. } => "Chat Invite",
                            };
                            let time_str = msg.created_at.format("%d %b %H:%M").to_string();
                            let body_preview = if msg.body.len() > 200 {
                                format!("{}...", &msg.body[..197])
                            } else {
                                msg.body.clone()
                            };
                            rsx! {
                                div { class: "messages-item", key: "{msg.id}",
                                    div { class: "messages-item-header",
                                        span { class: "messages-item-sender", "{msg.from_name}" }
                                        span { class: "messages-item-badge", "{kind_badge}" }
                                        span { class: "messages-item-time", "{time_str}" }
                                    }
                                    p { class: "messages-item-body", "{body_preview}" }
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
