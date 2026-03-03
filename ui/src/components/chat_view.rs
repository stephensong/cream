use dioxus::prelude::*;

use cream_common::chat::CHAT_MESSAGE_COST_CURD;

use super::chat_client::{ChatMessage, ChatSession, ChatState, ChatWsHandle, ClientMsg, SessionStatus};
#[cfg(target_family = "wasm")]
use super::chat_client::WebRtcSessions;
use super::node_api::{use_node_action, NodeAction};
use super::shared_state::use_shared_state;
use super::user_state::use_user_state;

fn clog(msg: &str) {
    #[cfg(target_family = "wasm")]
    web_sys::console::log_1(&msg.into());
    #[cfg(not(target_family = "wasm"))]
    let _ = msg;
}

/// Poll the relay every 5 seconds to check if a peer is online.
/// Returns `true` if the peer is currently connected to the relay.
#[allow(dead_code)] // used in WASM UI
pub fn use_peer_presence(pubkey: &Option<String>) -> bool {
    let chat = use_context::<Signal<ChatState>>();
    let ws_handle = use_context::<Signal<ChatWsHandle>>();

    // Store pubkey in a signal so the effect can track changes reactively.
    let mut pk_signal = use_signal(|| pubkey.clone());
    if *pk_signal.read() != *pubkey {
        pk_signal.set(pubkey.clone());
    }

    // Isolate authenticated from other ChatState fields so the effect doesn't
    // re-fire on every peer_online update (which would create a ping storm).
    let authenticated = use_memo(move || chat.read().authenticated);

    use_effect(move || {
        let pk = pk_signal.read().clone();
        let auth = *authenticated.read();

        let Some(pk) = pk else { return; };
        if !auth { return; }

        // Send an immediate ping
        ws_handle.peek().send(&ClientMsg::Ping { pubkey: pk.clone() });

        // Poll every 5s; uses peek() to avoid subscribing to signal changes
        spawn(async move {
            loop {
                #[cfg(target_family = "wasm")]
                gloo_timers::future::TimeoutFuture::new(5_000).await;
                #[cfg(not(target_family = "wasm"))]
                return;

                if !chat.peek().authenticated { break; }
                // Stop if pubkey changed (a new effect run handles the new pk)
                if pk_signal.peek().as_ref() != Some(&pk) { break; }
                ws_handle.peek().send(&ClientMsg::Ping { pubkey: pk.clone() });
            }
        });
    });

    pubkey.as_ref()
        .map(|pk| chat.read().is_peer_online(pk))
        .unwrap_or(false)
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
    try_datachannel: impl Fn(&str, &str) -> bool,
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

    // Try WebRTC data channel first, fall back to relay
    if try_datachannel(session_id, &body) {
        clog("[CHAT] Sent via WebRTC data channel");
        msg_input.set(String::new());
        return;
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
    #[cfg(target_family = "wasm")]
    let webrtc = use_context::<Signal<WebRtcSessions>>();
    let mut active_session = use_signal(|| None::<String>);
    let mut msg_input = use_signal(String::new);
    let node = use_node_action();
    let user_state = use_user_state();
    let my_name = user_state.read().moniker.clone().unwrap_or_default();

    // Returns a closure that tries sending via WebRTC data channel.
    // On WASM, reads the WebRtcSessions signal. On native, always returns false.
    #[cfg(target_family = "wasm")]
    let try_dc = move || {
        let webrtc = webrtc;
        move |sid: &str, text: &str| -> bool {
            super::chat_client::send_via_datachannel(&webrtc.read(), sid, text)
        }
    };
    #[cfg(not(target_family = "wasm"))]
    let try_dc = || { |_sid: &str, _text: &str| -> bool { false } };

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

                        // Auto-accept incoming chat invites
                        if session.status == SessionStatus::InviteReceived {
                            {
                                let sid = session.session_id.clone();
                                ws_handle.read().send(&ClientMsg::Accept {
                                    session_id: sid.clone(),
                                    ecdh_pubkey: String::new(),
                                });
                                if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                    s.status = SessionStatus::Active;
                                }
                                clog(&format!("[CHAT] Auto-accepted invite for session {}", sid));
                            }
                        }
                    }

                    // A/V toggle controls + text input (only when active)
                    if session.status == SessionStatus::Active {
                        div { class: "chat-av-controls",
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: session.mic_enabled,
                                    onchange: {
                                        let sid = session.session_id.clone();
                                        move |evt: Event<FormData>| {
                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                s.mic_enabled = evt.checked();
                                            }
                                        }
                                    },
                                }
                                " Microphone"
                            }
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: session.speaker_enabled,
                                    onchange: {
                                        let sid = session.session_id.clone();
                                        move |evt: Event<FormData>| {
                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                s.speaker_enabled = evt.checked();
                                            }
                                        }
                                    },
                                }
                                " Speaker"
                            }
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: session.camera_enabled,
                                    onchange: {
                                        let sid = session.session_id.clone();
                                        move |evt: Event<FormData>| {
                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                s.camera_enabled = evt.checked();
                                            }
                                        }
                                    },
                                }
                                " Camera"
                            }
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: session.tv_enabled,
                                    onchange: {
                                        let sid = session.session_id.clone();
                                        move |evt: Event<FormData>| {
                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                s.tv_enabled = evt.checked();
                                            }
                                        }
                                    },
                                }
                                " TV"
                            }
                        }
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
                                            send_chat_message(&mut chat, &ws_handle, &node, &session_id, &my_name, &mut msg_input, try_dc());
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
                                        send_chat_message(&mut chat, &ws_handle, &node, &session_id, &my_name, &mut msg_input, try_dc());
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

/// Message compose widget for the storefront view: textarea with Send (DM)
/// and Request Chat buttons.
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

    let peer_online = use_peer_presence(&supplier_pubkey);

    let msg_empty = invite_msg.read().trim().is_empty();
    let connected = chat.read().connected;
    let send_disabled = msg_empty || !can_afford;
    let chat_disabled = send_disabled || supplier_pubkey.is_none() || !connected || !peer_online;

    rsx! {
        div { class: "chat-invite-input",
            textarea {
                class: "message-textarea",
                placeholder: "Message to {supplier_name}...",
                value: "{invite_msg}",
                oninput: move |evt| invite_msg.set(evt.value()),
            }
            div { class: "message-send-controls",
                button {
                    class: "chat-start-btn",
                    disabled: send_disabled,
                    onclick: {
                        let supplier_name = supplier_name.clone();
                        move |_| {
                            let body = invite_msg.read().trim().to_string();
                            if body.is_empty() { return; }
                            node.send(NodeAction::SendInboxMessage {
                                recipient_name: supplier_name.clone(),
                                body,
                                kind: cream_common::inbox::MessageKind::DirectMessage,
                            });
                            invite_msg.set(String::new());
                        }
                    },
                    "Send Message"
                }
                button {
                    class: "chat-start-btn request-chat-btn",
                    disabled: chat_disabled,
                    onclick: {
                        let supplier_name = supplier_name.clone();
                        let my_name = my_name.clone();
                        move |_| {
                            let body = invite_msg.read().trim().to_string();
                            if body.is_empty() { return; }

                            if let Some(ref pubkey) = supplier_pubkey {
                                node.send(NodeAction::ChatMessageToll);

                                let session_id = format!("chat-{}", chrono::Utc::now().timestamp_millis());
                                node.send(NodeAction::SendInboxMessage {
                                    recipient_name: supplier_name.clone(),
                                    body: body.clone(),
                                    kind: cream_common::inbox::MessageKind::ChatInvite {
                                        session_id: session_id.clone(),
                                    },
                                });

                                let session = ChatSession {
                                    session_id: session_id.clone(),
                                    peer_pubkey: pubkey.clone(),
                                    peer_name: supplier_name.clone(),
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
                                };
                                {
                                    let mut state = chat.write();
                                    state.sessions.insert(session_id.clone(), session);
                                    state.panel_open = true;
                                }

                                ws_handle.read().send(&ClientMsg::Invite {
                                    to: pubkey.clone(),
                                    session_id,
                                    ecdh_pubkey: String::new(),
                                    message: body,
                                });
                                invite_msg.set(String::new());
                                clog(&format!("[CHAT] Sent invite to {}", supplier_name));
                            }
                        }
                    },
                    "Request Chat"
                }
            }
        }
    }
}
