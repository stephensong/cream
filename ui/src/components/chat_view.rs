use dioxus::prelude::*;

use cream_common::chat::CHAT_MESSAGE_COST_CURD;

use super::chat_client::{ChatMessage, ChatSession, ChatState, ChatWsHandle, ClientMsg, SessionStatus};
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

fn clog(msg: &str) {
    #[cfg(target_family = "wasm")]
    web_sys::console::log_1(&msg.into());
    #[cfg(not(target_family = "wasm"))]
    let _ = msg;
}

/// Provide ChatState signal in the app context.
#[allow(dead_code)] // convenience accessor
pub fn use_chat_state() -> Signal<ChatState> {
    use_context::<Signal<ChatState>>()
}

/// Badge in the app header showing active chat count.
#[component]
pub fn ChatBadge() -> Element {
    let chat = use_context::<Signal<ChatState>>();
    let total = chat.read().sessions.len();

    if total == 0 {
        return rsx! {};
    }

    rsx! {
        span { class: "chat-badge",
            "Chat ({total})"
        }
    }
}

/// Prominent banner shown at the top of the app when there are incoming invites.
/// Visible on every route so the invitee never misses it.
#[component]
pub fn ChatInviteBanner() -> Element {
    let mut chat = use_context::<Signal<ChatState>>();

    // Collect invite-received sessions
    let invites: Vec<(String, String, String)> = chat.read().sessions.iter()
        .filter(|(_, s)| s.status == SessionStatus::InviteReceived)
        .map(|(sid, s)| {
            let preview = s.messages.first()
                .map(|m| {
                    if m.body.len() > 60 {
                        format!("{}...", &m.body[..57])
                    } else {
                        m.body.clone()
                    }
                })
                .unwrap_or_default();
            (sid.clone(), s.peer_name.clone(), preview)
        })
        .collect();

    if invites.is_empty() {
        return rsx! {};
    }

    rsx! {
        div { class: "chat-invite-banner",
            for (sid, peer_name, preview) in invites.iter() {
                {
                    let sid = sid.clone();
                    rsx! {
                        div { class: "chat-invite-banner-item",
                            key: "{sid}",
                            span { class: "chat-invite-banner-text",
                                strong { "{peer_name}" }
                                ": \"{preview}\""
                            }
                            button {
                                class: "chat-invite-banner-btn",
                                onclick: move |_| {
                                    chat.write().panel_open = true;
                                },
                                "Open Chat"
                            }
                        }
                    }
                }
            }
        }
    }
}

fn send_chat_message(
    chat: &mut Signal<ChatState>,
    ws_handle: &Signal<ChatWsHandle>,
    node: &Coroutine<NodeAction>,
    session_id: &str,
    my_name: &str,
    msg_input: &mut Signal<String>,
) {
    let body = msg_input.read().trim().to_string();
    if body.is_empty() { return; }

    // Charge per-message toll
    node.send(NodeAction::ChatMessageToll);

    let msg = ChatMessage {
        sender_is_me: true,
        sender_name: my_name.to_string(),
        body: body.clone(),
        timestamp: chrono::Utc::now(),
    };
    if let Some(s) = chat.write().sessions.get_mut(session_id) {
        s.messages.push(msg);
    }

    ws_handle.read().send(&ClientMsg::Text {
        session_id: session_id.to_string(),
        ciphertext: body,
        nonce: String::new(),
    });
    msg_input.set(String::new());
}

/// Floating chat panel — slide-in from right side.
#[component]
pub fn ChatPanel() -> Element {
    let mut chat = use_context::<Signal<ChatState>>();
    let ws_handle = use_context::<Signal<ChatWsHandle>>();
    let mut active_session = use_signal(|| None::<String>);
    let mut msg_input = use_signal(String::new);
    let node = use_node_action();
    let user_state = use_user_state();
    let my_name = user_state.read().moniker.clone().unwrap_or_default();

    let chat_read = chat.read();
    let has_sessions = !chat_read.sessions.is_empty();
    let is_open = chat_read.panel_open;
    drop(chat_read);

    // Toggle button (always visible when there are sessions)
    if !is_open && !has_sessions {
        return rsx! {};
    }

    if !is_open {
        return rsx! {
            button {
                class: "chat-toggle-btn",
                onclick: move |_| { chat.write().panel_open = true; },
                "Chat"
                {
                    let total = chat.read().sessions.len();
                    if total > 0 {
                        rsx! { span { class: "chat-count", " ({total})" } }
                    } else {
                        rsx! {}
                    }
                }
            }
        };
    }

    // Get active session data
    let current_session_id = active_session.read().clone()
        .or_else(|| chat.read().sessions.keys().next().cloned());

    let session_data: Option<ChatSession> = current_session_id.as_ref()
        .and_then(|sid| chat.read().sessions.get(sid).cloned());

    let sessions_list: Vec<(String, String)> = chat.read().sessions.iter()
        .map(|(sid, s)| (sid.clone(), s.peer_name.clone()))
        .collect();

    rsx! {
        div { class: "chat-panel",
            div { class: "chat-panel-header",
                h3 { "Chat" }
                button {
                    class: "chat-close-btn",
                    onclick: move |_| { chat.write().panel_open = false; },
                    "X"
                }
            }

            // Session list
            if sessions_list.len() > 1 {
                div { class: "chat-session-list",
                    for (sid, peer_name) in sessions_list.iter() {
                        {
                            let sid_clone = sid.clone();
                            let is_active = current_session_id.as_ref() == Some(sid);
                            rsx! {
                                button {
                                    class: if is_active { "chat-session-tab active" } else { "chat-session-tab" },
                                    onclick: move |_| active_session.set(Some(sid_clone.clone())),
                                    "{peer_name}"
                                }
                            }
                        }
                    }
                }
            }

            // Active chat thread
            if let Some(session) = session_data {
                div { class: "chat-thread",
                    // Messages
                    div { class: "chat-messages",
                        for msg in session.messages.iter() {
                            {
                                let bubble_class = if msg.sender_is_me {
                                    "chat-bubble chat-sent"
                                } else {
                                    "chat-bubble chat-received"
                                };
                                let time_str = msg.timestamp.format("%H:%M").to_string();
                                rsx! {
                                    div { class: "{bubble_class}",
                                        div { class: "chat-sender-line",
                                            span { class: "chat-sender", "{msg.sender_name}" }
                                            span { class: "chat-time", "{time_str}" }
                                        }
                                        p { "{msg.body}" }
                                    }
                                }
                            }
                        }

                        // Pending accept notice (inviter side)
                        if session.status == SessionStatus::PendingAccept {
                            p { class: "chat-pending-notice", "Waiting for response..." }
                        }

                        // Invite received: Accept button (invitee side)
                        if session.status == SessionStatus::InviteReceived {
                            {
                                let sid = session.session_id.clone();
                                rsx! {
                                    button {
                                        class: "chat-invite-accept",
                                        onclick: move |_| {
                                            // Send accept to relay
                                            ws_handle.read().send(&ClientMsg::Accept {
                                                session_id: sid.clone(),
                                                ecdh_pubkey: String::new(),
                                            });
                                            // Update session status
                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                s.status = SessionStatus::Active;
                                            }
                                            clog(&format!("[CHAT] Accepted invite for session {}", sid));
                                        },
                                        "Accept"
                                    }
                                }
                            }
                        }
                    }

                    // Input + Send button (only when active)
                    if session.status == SessionStatus::Active {
                        div { class: "chat-input",
                            input {
                                r#type: "text",
                                placeholder: "Type a message...",
                                value: "{msg_input}",
                                oninput: move |evt| msg_input.set(evt.value()),
                                onkeypress: {
                                    let session_id = session.session_id.clone();
                                    let my_name = my_name.clone();
                                    move |evt: KeyboardEvent| {
                                        if evt.key() == Key::Enter {
                                            send_chat_message(&mut chat, &ws_handle, &node, &session_id, &my_name, &mut msg_input);
                                        }
                                    }
                                },
                            }
                            button {
                                class: "chat-send-btn",
                                disabled: msg_input.read().trim().is_empty(),
                                onclick: {
                                    let session_id = session.session_id.clone();
                                    let my_name = my_name.clone();
                                    move |_| {
                                        send_chat_message(&mut chat, &ws_handle, &node, &session_id, &my_name, &mut msg_input);
                                    }
                                },
                                "Send"
                            }
                        }
                    }
                }
            } else if has_sessions {
                p { class: "chat-empty", "Select a session" }
            } else {
                p { class: "chat-empty", "No active chats" }
            }
        }
    }
}

/// Invite input + "Send Invite" button for the storefront view.
#[component]
pub fn ChatWithSupplierButton(supplier_name: String) -> Element {
    let shared = use_shared_state();
    let mut chat = use_context::<Signal<ChatState>>();
    let ws_handle = use_context::<Signal<ChatWsHandle>>();
    let node = use_node_action();
    let user_state = use_user_state();
    let my_name = user_state.read().moniker.clone().unwrap_or_default();
    let mut invite_msg = use_signal(String::new);

    let balance = shared.read().user_contract.as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
    let cost = CHAT_MESSAGE_COST_CURD;
    let can_afford = balance >= cost;

    // Find supplier's public key from directory
    let supplier_pubkey: Option<String> = {
        let shared_read = shared.read();
        shared_read.directory.entries.values()
            .find(|e| e.name == supplier_name)
            .map(|e| {
                let bytes = e.supplier.0.to_bytes();
                bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>()
            })
    };

    let msg_empty = invite_msg.read().trim().is_empty();
    let connected = chat.read().connected;

    rsx! {
        div { class: "chat-invite-input",
            input {
                r#type: "text",
                placeholder: "Message to {supplier_name}...",
                value: "{invite_msg}",
                oninput: move |evt| invite_msg.set(evt.value()),
            }
            button {
                class: "chat-start-btn",
                disabled: msg_empty || !can_afford || supplier_pubkey.is_none() || !connected,
                title: if !can_afford {
                    format!("Need {} CURD", cost)
                } else if !connected {
                    "Not connected to chat relay".to_string()
                } else if msg_empty {
                    "Type a message first".to_string()
                } else {
                    format!("Send invite ({} CURD per message)", cost)
                },
                onclick: {
                    let supplier_name = supplier_name.clone();
                    let my_name = my_name.clone();
                    move |_| {
                        let body = invite_msg.read().trim().to_string();
                        if body.is_empty() { return; }

                        if let Some(ref pubkey) = supplier_pubkey {
                            // Charge one message toll for the invite message
                            node.send(NodeAction::ChatMessageToll);

                            // Also persist the invite as an inbox message so the
                            // recipient sees it even if they're offline.
                            let session_id = format!("chat-{}", chrono::Utc::now().timestamp_millis());
                            node.send(NodeAction::SendInboxMessage {
                                recipient_name: supplier_name.clone(),
                                body: body.clone(),
                                kind: cream_common::inbox::MessageKind::ChatInvite {
                                    session_id: session_id.clone(),
                                },
                            });

                            // Create session with PendingAccept status
                            let peer = pubkey.clone();
                            let invite_message = ChatMessage {
                                sender_is_me: true,
                                sender_name: my_name.clone(),
                                body: body.clone(),
                                timestamp: chrono::Utc::now(),
                            };
                            let session = ChatSession {
                                session_id: session_id.clone(),
                                peer_pubkey: peer.clone(),
                                peer_name: supplier_name.clone(),
                                messages: vec![invite_message],
                                started_at: chrono::Utc::now(),
                                status: SessionStatus::PendingAccept,
                                has_av: false,
                            };
                            {
                                let mut state = chat.write();
                                state.sessions.insert(session_id.clone(), session);
                                state.panel_open = true;
                            }

                            // Send invite via WebSocket with message
                            ws_handle.read().send(&ClientMsg::Invite {
                                to: peer,
                                session_id,
                                ecdh_pubkey: String::new(),
                                message: body,
                            });
                            invite_msg.set(String::new());
                            clog(&format!("[CHAT] Sent invite to {}", supplier_name));
                        }
                    }
                },
                "Send Invite"
            }
        }
    }
}
