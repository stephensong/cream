use dioxus::prelude::*;

use cream_common::chat::{CHAT_TEXT_DEPOSIT_CURD, CHAT_SESSION_MINUTES};

use super::chat_client::{ChatMessage, ChatSession, ChatState, PendingInvite};
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;

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

/// Badge in the app header showing active chat count + incoming invites.
#[component]
pub fn ChatBadge() -> Element {
    let chat = use_context::<Signal<ChatState>>();
    let chat_read = chat.read();

    let session_count = chat_read.sessions.len();
    let invite_count = chat_read.pending_invites.len();
    let total = session_count + invite_count;

    if total == 0 {
        return rsx! {};
    }

    rsx! {
        span { class: "chat-badge",
            "Chat ({total})"
        }
    }
}

/// Toast notification for incoming chat invites.
#[component]
pub fn ChatInviteToast() -> Element {
    let mut chat = use_context::<Signal<ChatState>>();

    let invites: Vec<PendingInvite> = chat.read().pending_invites.clone();

    if invites.is_empty() {
        return rsx! {};
    }

    rsx! {
        div { class: "chat-invite-toasts",
            for invite in invites.iter() {
                {
                    let session_id = invite.session_id.clone();
                    let session_id_decline = invite.session_id.clone();
                    let ecdh_pubkey = invite.ecdh_pubkey.clone();
                    let from_short = if invite.from.len() > 16 {
                        format!("{}...{}", &invite.from[..8], &invite.from[invite.from.len()-8..])
                    } else {
                        invite.from.clone()
                    };
                    rsx! {
                        div { class: "chat-invite-toast",
                            key: "{invite.session_id}",
                            p { "Chat invite from {from_short}" }
                            div { class: "chat-invite-actions",
                                button {
                                    class: "chat-accept-btn",
                                    onclick: move |_| {
                                        accept_invite(&mut chat, &session_id, &ecdh_pubkey);
                                    },
                                    "Accept"
                                }
                                button {
                                    class: "chat-decline-btn",
                                    onclick: move |_| {
                                        decline_invite(&mut chat, &session_id_decline);
                                    },
                                    "Decline"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn accept_invite(chat: &mut Signal<ChatState>, session_id: &str, _ecdh_pubkey: &str) {
    let mut state = chat.write();
    // Remove from pending
    let invite = state.pending_invites.iter()
        .find(|i| i.session_id == session_id)
        .cloned();

    if let Some(invite) = invite {
        state.pending_invites.retain(|i| i.session_id != session_id);

        // Create session
        let session = ChatSession {
            session_id: session_id.to_string(),
            peer_pubkey: invite.from.clone(),
            messages: Vec::new(),
            started_at: chrono::Utc::now(),
            deposit_paid: 0, // Acceptor doesn't pay deposit in this version
            has_av: false,
        };
        state.sessions.insert(session_id.to_string(), session);
    }

    // Send accept via WebSocket (handled in the chat connection loop)
    clog(&format!("[CHAT] Accepted invite for session {}", session_id));
}

fn decline_invite(chat: &mut Signal<ChatState>, session_id: &str) {
    chat.write().pending_invites.retain(|i| i.session_id != session_id);
    clog(&format!("[CHAT] Declined invite for session {}", session_id));
}

/// Floating chat panel — slide-in from right side.
#[component]
pub fn ChatPanel() -> Element {
    let mut chat = use_context::<Signal<ChatState>>();
    let mut panel_open = use_signal(|| false);
    let mut active_session = use_signal(|| None::<String>);
    let mut msg_input = use_signal(String::new);
    let _shared = use_shared_state();
    let node = use_node_action();

    let chat_read = chat.read();
    let has_sessions = !chat_read.sessions.is_empty();
    let has_invites = !chat_read.pending_invites.is_empty();
    let is_open = *panel_open.read();
    drop(chat_read);

    // Toggle button (always visible when there are sessions)
    if !is_open && !has_sessions && !has_invites {
        return rsx! {};
    }

    if !is_open {
        return rsx! {
            button {
                class: "chat-toggle-btn",
                onclick: move |_| panel_open.set(true),
                "Chat"
                {
                    let total = chat.read().sessions.len() + chat.read().pending_invites.len();
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
        .map(|(sid, s)| {
            let peer_short = if s.peer_pubkey.len() > 16 {
                format!("{}..{}", &s.peer_pubkey[..8], &s.peer_pubkey[s.peer_pubkey.len()-8..])
            } else {
                s.peer_pubkey.clone()
            };
            (sid.clone(), peer_short)
        })
        .collect();

    rsx! {
        div { class: "chat-panel",
            div { class: "chat-panel-header",
                h3 { "Chat" }
                button {
                    class: "chat-close-btn",
                    onclick: move |_| panel_open.set(false),
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
                    // Timer
                    div { class: "chat-timer",
                        {
                            let elapsed = (chrono::Utc::now() - session.started_at).num_seconds().max(0) as u64;
                            let total_secs = CHAT_SESSION_MINUTES * 60;
                            let remaining = total_secs.saturating_sub(elapsed);
                            let mins = remaining / 60;
                            let secs = remaining % 60;
                            rsx! { span { class: "timer-text", "{mins}:{secs:02}" } }
                        }
                    }

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
                                        p { "{msg.body}" }
                                        span { class: "chat-time", "{time_str}" }
                                    }
                                }
                            }
                        }
                    }

                    // Input
                    div { class: "chat-input",
                        input {
                            r#type: "text",
                            placeholder: "Type a message...",
                            value: "{msg_input}",
                            oninput: move |evt| msg_input.set(evt.value()),
                            onkeypress: {
                                let session_id = session.session_id.clone();
                                move |evt: KeyboardEvent| {
                                    if evt.key() == Key::Enter {
                                        let body = msg_input.read().trim().to_string();
                                        if body.is_empty() { return; }

                                        // Add to local messages
                                        let msg = ChatMessage {
                                            sender_is_me: true,
                                            body: body.clone(),
                                            timestamp: chrono::Utc::now(),
                                        };
                                        if let Some(s) = chat.write().sessions.get_mut(&session_id) {
                                            s.messages.push(msg);
                                        }

                                        // TODO: Send via WebSocket (encrypted)
                                        clog(&format!("[CHAT] Send text: {}", body));
                                        msg_input.set(String::new());
                                    }
                                }
                            },
                        }
                    }

                    // End chat button
                    div { class: "chat-actions",
                        button {
                            class: "chat-end-btn",
                            onclick: {
                                let session_id = session.session_id.clone();
                                let deposit = session.deposit_paid;
                                let started = session.started_at;
                                move |_| {
                                    let elapsed = (chrono::Utc::now() - started).num_seconds().max(0) as u64;

                                    // Issue refund for unused time
                                    if deposit > 0 {
                                        node.send(NodeAction::ChatRefund {
                                            elapsed_secs: elapsed,
                                            deposit,
                                            session_minutes: CHAT_SESSION_MINUTES,
                                        });
                                    }

                                    // Remove session
                                    chat.write().sessions.remove(&session_id);
                                    active_session.set(None);
                                    clog(&format!("[CHAT] Ended session {}", session_id));
                                }
                            },
                            "End Chat"
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

/// "Chat with Supplier" button for the storefront view.
#[component]
pub fn ChatWithSupplierButton(supplier_name: String) -> Element {
    let shared = use_shared_state();
    let mut chat = use_context::<Signal<ChatState>>();
    let node = use_node_action();

    let balance = shared.read().user_contract.as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
    let deposit = CHAT_TEXT_DEPOSIT_CURD;
    let can_afford = balance >= deposit;

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

    rsx! {
        button {
            class: "chat-start-btn",
            disabled: !can_afford || supplier_pubkey.is_none() || !chat.read().connected,
            title: if !can_afford {
                format!("Need {} CURD", deposit)
            } else if !chat.read().connected {
                "Not connected to chat relay".to_string()
            } else {
                format!("Start private chat ({} CURD deposit)", deposit)
            },
            onclick: {
                let supplier_name = supplier_name.clone();
                move |_| {
                    if let Some(ref pubkey) = supplier_pubkey {
                        // Pay deposit
                        node.send(NodeAction::ChatDeposit { amount: deposit });

                        // Create session
                        let session_id = format!("chat-{}", chrono::Utc::now().timestamp_millis());
                        let peer = pubkey.clone();
                        let session = ChatSession {
                            session_id: session_id.clone(),
                            peer_pubkey: peer,
                            messages: Vec::new(),
                            started_at: chrono::Utc::now(),
                            deposit_paid: deposit,
                            has_av: false,
                        };
                        chat.write().sessions.insert(session_id.clone(), session);

                        // TODO: Send invite via WebSocket
                        clog(&format!("[CHAT] Started chat with {} (session {})", supplier_name, session_id));
                    }
                }
            },
            "Chat with {supplier_name} ({deposit} CURD)"
        }
    }
}
