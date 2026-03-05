use dioxus::prelude::*;

use super::toll_rates::use_toll_rates;

use super::chat_client::{ChatMessage, ChatSession, ChatState, ChatWsHandle, ClientMsg, PaymentRequest, SessionStatus};
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
    _node: &Coroutine<NodeAction>,
    session_id: &str,
    my_name: &str,
    msg_input: &mut Signal<String>,
    try_datachannel: impl Fn(&str, &str) -> bool,
) {
    let body = msg_input.read().trim().to_string();
    if body.is_empty() { return; }

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
    let mut pay_amount_input = use_signal(String::new);
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

    // Unified payment loop: session tolls (initiator-only) + request-to-pay
    let shared = use_shared_state();
    let toll_rates = use_toll_rates();
    {
        let node = node.clone();
        use_effect(move || {
            spawn(async move {
                loop {
                    #[cfg(target_family = "wasm")]
                    {
                        let interval_ms = toll_rates.read().session_interval_secs * 1000;
                        gloo_timers::future::TimeoutFuture::new(interval_ms).await;
                    }
                    #[cfg(not(target_family = "wasm"))]
                    return;

                    let balance = shared.peek().user_contract
                        .as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
                    let toll_cost = toll_rates.read().session_toll_curd;

                    // Step A — Session tolls: initiator pays per interval
                    let sessions: Vec<(String, bool, String)> = chat.peek().sessions.iter()
                        .filter(|(_, s)| s.status == SessionStatus::Active && s.is_initiator)
                        .map(|(sid, s)| (sid.clone(), true, s.peer_pubkey.clone()))
                        .collect();

                    for (sid, _, _) in &sessions {
                        if balance < toll_cost {
                            clog(&format!("[CHAT] Session toll: insufficient balance, closing session {}", sid));
                            ws_handle.peek().send(&ClientMsg::Close {
                                session_id: sid.clone(),
                            });
                            chat.write().sessions.remove(sid);
                        } else {
                            node.send(NodeAction::SessionToll);
                        }
                    }

                    // Step B — Request-to-pay: charge peer transfers
                    let paying_sessions: Vec<(String, u64, String)> = chat.peek().sessions.iter()
                        .filter_map(|(sid, s)| {
                            if let Some(PaymentRequest::ActivePaying { curd_per_interval }) = &s.payment_request {
                                Some((sid.clone(), *curd_per_interval, s.peer_pubkey.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();

                    for (sid, amount, peer_pubkey) in &paying_sessions {
                        let current_balance = shared.peek().user_contract
                            .as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
                        if current_balance < *amount {
                            clog(&format!("[CHAT] Request-to-pay: insufficient balance, stopping payment for {}", sid));
                            // Send stop via data channel
                            #[cfg(target_family = "wasm")]
                            {
                                let rtc_sessions = webrtc.read();
                                let ctrl_msg = format!("{}pay_stop", super::chat_client::DC_CONTROL_PREFIX);
                                super::chat_client::send_via_datachannel(&rtc_sessions, sid, &ctrl_msg);
                            }
                            if let Some(s) = chat.write().sessions.get_mut(sid) {
                                s.payment_request = None;
                            }
                        } else {
                            node.send(NodeAction::PeerTransfer {
                                peer_pubkey_hex: peer_pubkey.clone(),
                                amount: *amount,
                                description: "Chat peer payment".to_string(),
                            });
                        }
                    }
                }
            });
        });
    }

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

                    // Remote video panel — visible when TV is toggled on.
                    // Shows the video feed if the peer has their camera on,
                    // otherwise shows a "No camera feed" placeholder.
                    if session.tv_enabled {
                        div { class: "chat-remote-video",
                            if session.has_remote_video {
                                video {
                                    id: "remote-video-el-{session.session_id}",
                                    autoplay: true,
                                    muted: !session.speaker_enabled,
                                    playsinline: "",
                                    onmounted: {
                                        #[cfg(target_family = "wasm")]
                                        let sid = session.session_id.clone();
                                        #[cfg(target_family = "wasm")]
                                        let webrtc = webrtc;
                                        move |_| {
                                            #[cfg(target_family = "wasm")]
                                            {
                                                let rtc_sessions = webrtc.read();
                                                if let Some(rtc) = rtc_sessions.get(&sid) {
                                                    if let Some(ref stream) = *rtc.remote_stream.lock().unwrap() {
                                                        super::chat_client::wasm::attach_remote_stream_to_video(&sid, stream);
                                                    }
                                                }
                                            }
                                        }
                                    },
                                }
                            } else {
                                p { class: "chat-no-video", "No camera feed" }
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
                                            let enabled = evt.checked();
                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                s.mic_enabled = enabled;
                                            }
                                            #[cfg(target_family = "wasm")]
                                            {
                                                let rtc_sessions = webrtc.read();
                                                if let Some(rtc) = rtc_sessions.get(&sid) {
                                                    if let Some(ref ws) = ws_handle.read().ws {
                                                        if enabled {
                                                            super::chat_client::wasm::start_mic(rtc, ws, &sid);
                                                        } else {
                                                            super::chat_client::wasm::stop_mic(rtc, ws, &sid);
                                                        }
                                                    }
                                                }
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
                                            let enabled = evt.checked();
                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                s.speaker_enabled = enabled;
                                            }
                                            #[cfg(target_family = "wasm")]
                                            {
                                                let rtc_sessions = webrtc.read();
                                                if let Some(rtc) = rtc_sessions.get(&sid) {
                                                    super::chat_client::wasm::set_remote_audio_enabled(rtc, enabled, &sid);
                                                }
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
                                            let enabled = evt.checked();
                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                s.camera_enabled = enabled;
                                            }
                                            #[cfg(target_family = "wasm")]
                                            {
                                                let rtc_sessions = webrtc.read();
                                                if let Some(rtc) = rtc_sessions.get(&sid) {
                                                    if let Some(ref ws) = ws_handle.read().ws {
                                                        if enabled {
                                                            super::chat_client::wasm::start_camera(rtc, ws, &sid);
                                                        } else {
                                                            super::chat_client::wasm::stop_camera(rtc, ws, &sid);
                                                        }
                                                    }
                                                }
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
                                " Screen"
                            }
                        }
                        // Request-to-pay controls
                        div { class: "chat-payment-controls",
                            {
                                let sid = session.session_id.clone();
                                let payment_request = session.payment_request.clone();
                                match payment_request {
                                    None => {
                                        let sid = sid.clone();
                                        rsx! {
                                            div { class: "chat-pay-request",
                                                input {
                                                    r#type: "number",
                                                    min: "1",
                                                    placeholder: "CURD/interval",
                                                    value: "{pay_amount_input}",
                                                    oninput: move |e| pay_amount_input.set(e.value()),
                                                }
                                                button {
                                                    class: "chat-pay-btn",
                                                    disabled: pay_amount_input.read().parse::<u64>().unwrap_or(0) == 0,
                                                    onclick: {
                                                        let sid = sid.clone();
                                                        move |_| {
                                                            let amount: u64 = pay_amount_input.read().parse().unwrap_or(0);
                                                            if amount == 0 { return; }
                                                            #[cfg(target_family = "wasm")]
                                                            {
                                                                let rtc_sessions = webrtc.read();
                                                                let ctrl_msg = format!("{}pay_request:{}", super::chat_client::DC_CONTROL_PREFIX, amount);
                                                                super::chat_client::send_via_datachannel(&rtc_sessions, &sid, &ctrl_msg);
                                                            }
                                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                                s.payment_request = Some(PaymentRequest::SentPending { curd_per_interval: amount });
                                                            }
                                                            pay_amount_input.set(String::new());
                                                        }
                                                    },
                                                    "Request Payment"
                                                }
                                            }
                                        }
                                    }
                                    Some(PaymentRequest::SentPending { curd_per_interval }) => {
                                        let sid = sid.clone();
                                        rsx! {
                                            div { class: "chat-pay-status",
                                                span { "Requested {curd_per_interval} CURD/interval..." }
                                                button {
                                                    class: "chat-pay-cancel",
                                                    onclick: {
                                                        let sid = sid.clone();
                                                        move |_| {
                                                            #[cfg(target_family = "wasm")]
                                                            {
                                                                let rtc_sessions = webrtc.read();
                                                                let ctrl_msg = format!("{}pay_stop", super::chat_client::DC_CONTROL_PREFIX);
                                                                super::chat_client::send_via_datachannel(&rtc_sessions, &sid, &ctrl_msg);
                                                            }
                                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                                s.payment_request = None;
                                                            }
                                                        }
                                                    },
                                                    "Cancel"
                                                }
                                            }
                                        }
                                    }
                                    Some(PaymentRequest::ReceivedPending { curd_per_interval }) => {
                                        let sid = sid.clone();
                                        rsx! {
                                            div { class: "chat-pay-status",
                                                span { "Peer requests {curd_per_interval} CURD/interval" }
                                                button {
                                                    class: "chat-pay-accept",
                                                    onclick: {
                                                        let sid = sid.clone();
                                                        move |_| {
                                                            #[cfg(target_family = "wasm")]
                                                            {
                                                                let rtc_sessions = webrtc.read();
                                                                let ctrl_msg = format!("{}pay_accept", super::chat_client::DC_CONTROL_PREFIX);
                                                                super::chat_client::send_via_datachannel(&rtc_sessions, &sid, &ctrl_msg);
                                                            }
                                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                                s.payment_request = Some(PaymentRequest::ActivePaying { curd_per_interval });
                                                            }
                                                        }
                                                    },
                                                    "Accept"
                                                }
                                                button {
                                                    class: "chat-pay-decline",
                                                    onclick: {
                                                        let sid = sid.clone();
                                                        move |_| {
                                                            #[cfg(target_family = "wasm")]
                                                            {
                                                                let rtc_sessions = webrtc.read();
                                                                let ctrl_msg = format!("{}pay_decline", super::chat_client::DC_CONTROL_PREFIX);
                                                                super::chat_client::send_via_datachannel(&rtc_sessions, &sid, &ctrl_msg);
                                                            }
                                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                                s.payment_request = None;
                                                            }
                                                        }
                                                    },
                                                    "Decline"
                                                }
                                            }
                                        }
                                    }
                                    Some(PaymentRequest::ActivePaying { curd_per_interval }) => {
                                        let sid = sid.clone();
                                        rsx! {
                                            div { class: "chat-pay-status chat-pay-active",
                                                span { "Paying {curd_per_interval} CURD/interval" }
                                                button {
                                                    class: "chat-pay-stop",
                                                    onclick: {
                                                        let sid = sid.clone();
                                                        move |_| {
                                                            #[cfg(target_family = "wasm")]
                                                            {
                                                                let rtc_sessions = webrtc.read();
                                                                let ctrl_msg = format!("{}pay_stop", super::chat_client::DC_CONTROL_PREFIX);
                                                                super::chat_client::send_via_datachannel(&rtc_sessions, &sid, &ctrl_msg);
                                                            }
                                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                                s.payment_request = None;
                                                            }
                                                        }
                                                    },
                                                    "Stop"
                                                }
                                            }
                                        }
                                    }
                                    Some(PaymentRequest::ActiveReceiving { curd_per_interval }) => {
                                        let sid = sid.clone();
                                        rsx! {
                                            div { class: "chat-pay-status chat-pay-active",
                                                span { "Receiving {curd_per_interval} CURD/interval" }
                                                button {
                                                    class: "chat-pay-stop",
                                                    onclick: {
                                                        let sid = sid.clone();
                                                        move |_| {
                                                            #[cfg(target_family = "wasm")]
                                                            {
                                                                let rtc_sessions = webrtc.read();
                                                                let ctrl_msg = format!("{}pay_stop", super::chat_client::DC_CONTROL_PREFIX);
                                                                super::chat_client::send_via_datachannel(&rtc_sessions, &sid, &ctrl_msg);
                                                            }
                                                            if let Some(s) = chat.write().sessions.get_mut(&sid) {
                                                                s.payment_request = None;
                                                            }
                                                        }
                                                    },
                                                    "Stop"
                                                }
                                            }
                                        }
                                    }
                                }
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

    let toll_rates_invite = use_toll_rates();
    let balance = shared.read().user_contract.as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
    let cost = toll_rates_invite.read().session_toll_curd;
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
                                recipient_pubkey_hex: None,
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
                                let session_id = format!("chat-{}", chrono::Utc::now().timestamp_millis());
                                node.send(NodeAction::SendInboxMessage {
                                    recipient_name: supplier_name.clone(),
                                    body: body.clone(),
                                    kind: cream_common::inbox::MessageKind::ChatInvite {
                                        session_id: session_id.clone(),
                                    },
                                    recipient_pubkey_hex: None,
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
