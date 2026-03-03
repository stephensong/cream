use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(target_family = "wasm")]
use std::sync::{Arc, Mutex};

/// Default relay URL, overridden at compile-time via CREAM_RELAY_URL.
#[allow(dead_code)] // used in WASM builds
pub fn relay_url() -> String {
    option_env!("CREAM_RELAY_URL")
        .unwrap_or("ws://localhost:3020")
        .to_string()
}

// ---------- Protocol types (mirror relay/src/types.ts) ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)] // used in WASM relay communication
pub enum ServerMessage {
    Nonce { nonce: String },
    AuthOk,
    Error { message: String },
    Invite { from: String, session_id: String, ecdh_pubkey: String, message: String },
    Accept { session_id: String, ecdh_pubkey: String },
    Decline { session_id: String },
    Text { session_id: String, ciphertext: String, nonce: String },
    Sdp { session_id: String, sdp: serde_json::Value },
    Ice { session_id: String, candidate: serde_json::Value },
    Close { session_id: String, reason: String },
    Presence { pubkey: String, online: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)] // used in WASM relay communication
pub enum ClientMsg {
    Auth { public_key: String, signature: String, nonce: String },
    Invite { to: String, session_id: String, ecdh_pubkey: String, message: String },
    Accept { session_id: String, ecdh_pubkey: String },
    Decline { session_id: String },
    Text { session_id: String, ciphertext: String, nonce: String },
    Sdp { session_id: String, sdp: serde_json::Value },
    Ice { session_id: String, candidate: serde_json::Value },
    Close { session_id: String },
    Ping { pubkey: String },
}

// ---------- Session status ----------

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    PendingAccept,   // inviter waiting for response
    InviteReceived,  // invitee sees invite, hasn't accepted yet
    Active,          // both parties accepted, chat is live
}

// ---------- Chat session ----------

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub sender_is_me: bool,
    pub sender_name: String,
    pub body: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields used in WASM UI
pub struct ChatSession {
    pub session_id: String,
    pub peer_pubkey: String,
    pub peer_name: String,
    pub messages: Vec<ChatMessage>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub status: SessionStatus,
    pub mic_enabled: bool,
    pub speaker_enabled: bool,
    pub camera_enabled: bool,
    pub tv_enabled: bool,
    pub has_remote_video: bool,
}

// ---------- Chat state (shared via Signal in UI) ----------

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // fields used in WASM UI
pub struct ChatState {
    pub connected: bool,
    pub authenticated: bool,
    pub sessions: HashMap<String, ChatSession>,
    pub panel_open: bool,
    pub last_error: Option<String>,
    pub peer_online: HashMap<String, bool>,
}

#[allow(dead_code)]
impl ChatState {
    pub fn is_peer_online(&self, pubkey: &str) -> bool {
        self.peer_online.get(pubkey).copied().unwrap_or(false)
    }
}

// ---------- WebRTC data channel session ----------

/// Tracks a WebRTC peer connection and its data channel for one chat session.
/// Stored outside of ChatState because RtcPeerConnection is not Clone.
#[cfg(target_family = "wasm")]
pub struct WebRtcSession {
    pub pc: web_sys::RtcPeerConnection,
    pub dc: Arc<Mutex<Option<web_sys::RtcDataChannel>>>,
    pub ready: Arc<Mutex<bool>>,
    pub local_audio_track: Arc<Mutex<Option<web_sys::MediaStreamTrack>>>,
    pub local_video_track: Arc<Mutex<Option<web_sys::MediaStreamTrack>>>,
    pub remote_stream: Arc<Mutex<Option<web_sys::MediaStream>>>,
    pub audio_sender: Arc<Mutex<Option<web_sys::RtcRtpSender>>>,
    pub video_sender: Arc<Mutex<Option<web_sys::RtcRtpSender>>>,
}

/// Map of session_id → WebRtcSession. Shared via Dioxus context as Signal.
#[cfg(target_family = "wasm")]
pub type WebRtcSessions = HashMap<String, WebRtcSession>;

/// Prefix for internal control messages sent over the data channel.
/// These are not displayed as chat messages.
pub const DC_CONTROL_PREFIX: &str = "__ctrl:";

/// Try to send a message via the WebRTC data channel for this session.
/// Returns true if sent, false if data channel not ready (caller should fall back to relay).
#[cfg(target_family = "wasm")]
pub fn send_via_datachannel(sessions: &WebRtcSessions, session_id: &str, text: &str) -> bool {
    if let Some(session) = sessions.get(session_id) {
        let ready = *session.ready.lock().unwrap();
        if ready {
            if let Some(ref dc) = *session.dc.lock().unwrap() {
                if dc.ready_state() == web_sys::RtcDataChannelState::Open {
                    if dc.send_with_str(text).is_ok() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------- Shared WebSocket handle ----------

/// Wraps a WebSocket connection for sharing via Dioxus context.
/// On WASM, holds a `web_sys::WebSocket`. On native, a unit stub.
#[derive(Clone, Default)]
pub struct ChatWsHandle {
    #[cfg(target_family = "wasm")]
    pub ws: Option<web_sys::WebSocket>,
    #[cfg(not(target_family = "wasm"))]
    pub ws: Option<()>,
}

#[allow(dead_code)]
impl ChatWsHandle {
    pub fn is_connected(&self) -> bool {
        self.ws.is_some()
    }

    /// Send a client message. No-op if not connected.
    pub fn send(&self, msg: &ClientMsg) {
        if let Some(ref ws) = self.ws {
            if let Err(e) = wasm::send_msg(ws, msg) {
                #[cfg(target_family = "wasm")]
                web_sys::console::log_1(&format!("[CHAT] Send error: {}", e).into());
                #[cfg(not(target_family = "wasm"))]
                let _ = e;
            }
        }
    }
}

// ---------- WASM WebSocket client ----------

#[cfg(target_family = "wasm")]
#[allow(dead_code)] // functions available for chat_view integration
pub mod wasm {
    use super::*;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::WebSocket;

    fn clog(msg: &str) {
        web_sys::console::log_1(&msg.into());
    }

    /// Connect to the relay, authenticate with the provided signing key, and return the WebSocket.
    /// `signing_key_bytes` is the 32-byte ed25519 secret key used to sign the auth nonce.
    pub fn connect(
        relay_url: &str,
        pubkey_hex: &str,
        signing_key_bytes: [u8; 32],
        on_message: impl FnMut(ServerMessage) + 'static,
        on_open: impl FnMut() + 'static,
        on_close: impl FnMut() + 'static,
    ) -> Result<WebSocket, String> {
        let ws = WebSocket::new(relay_url)
            .map_err(|e| format!("WebSocket connect failed: {:?}", e))?;

        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let pubkey = pubkey_hex.to_string();

        // onopen
        let ws_clone = ws.clone();
        let mut on_open = on_open;
        let on_open_cb = Closure::wrap(Box::new(move |_: JsValue| {
            clog("[CHAT] WebSocket connected, waiting for nonce...");
            let _ = &ws_clone; // keep alive
            on_open();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(on_open_cb.as_ref().unchecked_ref()));
        on_open_cb.forget();

        // onmessage — handle auth handshake and forward messages
        let ws_clone = ws.clone();
        let pubkey_clone = pubkey.clone();
        let mut on_message = on_message;
        let on_msg_cb = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            let data = match event.data().as_string() {
                Some(s) => s,
                None => return,
            };

            let msg: ServerMessage = match serde_json::from_str(&data) {
                Ok(m) => m,
                Err(e) => {
                    clog(&format!("[CHAT] Failed to parse server message: {}", e));
                    return;
                }
            };

            // Auto-handle nonce -> sign and authenticate
            if let ServerMessage::Nonce { ref nonce } = msg {
                clog(&format!("[CHAT] Got nonce, signing with key {}..{}", &pubkey_clone[..8], &pubkey_clone[pubkey_clone.len()-8..]));
                let signature = sign_nonce(&signing_key_bytes, nonce);
                let auth = ClientMsg::Auth {
                    public_key: pubkey_clone.clone(),
                    signature,
                    nonce: nonce.clone(),
                };
                let json = serde_json::to_string(&auth).unwrap();
                let _ = ws_clone.send_with_str(&json);
                return;
            }

            on_message(msg);
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        ws.set_onmessage(Some(on_msg_cb.as_ref().unchecked_ref()));
        on_msg_cb.forget();

        // onclose
        let mut on_close = on_close;
        let on_close_cb = Closure::wrap(Box::new(move |_: JsValue| {
            clog("[CHAT] WebSocket closed");
            on_close();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onclose(Some(on_close_cb.as_ref().unchecked_ref()));
        on_close_cb.forget();

        // onerror
        let on_err_cb = Closure::wrap(Box::new(move |e: JsValue| {
            clog(&format!("[CHAT] WebSocket error: {:?}", e));
        }) as Box<dyn Fn(JsValue)>);
        ws.set_onerror(Some(on_err_cb.as_ref().unchecked_ref()));
        on_err_cb.forget();

        Ok(ws)
    }

    /// Send a client message over the WebSocket.
    pub fn send_msg(ws: &WebSocket, msg: &ClientMsg) -> Result<(), String> {
        let json = serde_json::to_string(msg).map_err(|e| format!("Serialize error: {}", e))?;
        ws.send_with_str(&json).map_err(|e| format!("Send error: {:?}", e))
    }

    /// Sign a nonce synchronously using the provided ed25519 signing key bytes.
    fn sign_nonce(key_bytes: &[u8; 32], nonce: &str) -> String {
        use ed25519_dalek::{Signer, SigningKey};

        let signing_key = SigningKey::from_bytes(key_bytes);
        let signature = signing_key.sign(nonce.as_bytes());
        bytes_to_hex(&signature.to_bytes())
    }

    fn bytes_to_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    // ---------- WebRTC helpers ----------

    use super::{WebRtcSession, Arc, Mutex};

    /// ICE server configuration. Reads CREAM_ICE_SERVERS env var at compile time.
    /// Falls back to Google STUN. Operators can set the env var to add TURN servers.
    fn ice_server_config() -> web_sys::RtcConfiguration {
        let json_str = option_env!("CREAM_ICE_SERVERS")
            .unwrap_or(r#"[{"urls":"stun:stun.l.google.com:19302"}]"#);

        let servers: Vec<serde_json::Value> = serde_json::from_str(json_str)
            .unwrap_or_else(|_| vec![serde_json::json!({"urls": "stun:stun.l.google.com:19302"})]);

        let ice_servers = js_sys::Array::new();
        for server in &servers {
            let obj = js_sys::Object::new();
            if let Some(urls) = server.get("urls") {
                let _ = js_sys::Reflect::set(
                    &obj,
                    &"urls".into(),
                    &serde_wasm_bindgen::to_value(urls).unwrap_or("stun:stun.l.google.com:19302".into()),
                );
            }
            if let Some(username) = server.get("username").and_then(|v| v.as_str()) {
                let _ = js_sys::Reflect::set(&obj, &"username".into(), &username.into());
            }
            if let Some(credential) = server.get("credential").and_then(|v| v.as_str()) {
                let _ = js_sys::Reflect::set(&obj, &"credential".into(), &credential.into());
            }
            ice_servers.push(&obj);
        }

        let config = web_sys::RtcConfiguration::new();
        config.set_ice_servers(&ice_servers);
        config
    }

    /// Create an RTC peer connection with configured ICE servers.
    pub fn create_peer_connection() -> Result<web_sys::RtcPeerConnection, String> {
        let config = ice_server_config();
        web_sys::RtcPeerConnection::new_with_configuration(&config)
            .map_err(|e| format!("RtcPeerConnection failed: {:?}", e))
    }

    /// Set up the offerer side: creates PeerConnection + DataChannel, generates SDP offer,
    /// and sends offer + ICE candidates via the relay WebSocket.
    pub fn setup_offerer(
        session_id: String,
        ws: &web_sys::WebSocket,
        mut on_dc_message: impl FnMut(String, String) + 'static,
        on_dc_open: impl FnMut(String) + 'static,
        on_remote_video: impl FnMut(String, bool) + 'static,
    ) -> Result<WebRtcSession, String> {
        let pc = create_peer_connection()?;
        let dc_holder: Arc<Mutex<Option<web_sys::RtcDataChannel>>> = Arc::new(Mutex::new(None));
        let ready: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let remote_stream: Arc<Mutex<Option<web_sys::MediaStream>>> = Arc::new(Mutex::new(None));

        // ontrack — store remote media stream
        let remote_stream_clone = remote_stream.clone();
        let sid_for_track = session_id.clone();
        setup_ontrack_handler(&pc, remote_stream_clone, sid_for_track, on_remote_video);

        // Create data channel
        let dc_init = web_sys::RtcDataChannelInit::new();
        dc_init.set_ordered(true);
        let dc = pc.create_data_channel_with_data_channel_dict("chat", &dc_init);

        // Data channel onopen
        let ready_clone = ready.clone();
        let sid_clone = session_id.clone();
        let on_dc_open = std::cell::RefCell::new(Some(on_dc_open));
        let on_open_cb = Closure::wrap(Box::new(move |_: JsValue| {
            clog(&format!("[WEBRTC] Data channel open for session {}", sid_clone));
            *ready_clone.lock().unwrap() = true;
            if let Some(mut cb) = on_dc_open.borrow_mut().take() {
                cb(sid_clone.clone());
            }
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onopen(Some(on_open_cb.as_ref().unchecked_ref()));
        on_open_cb.forget();

        // Data channel onmessage
        let sid_clone = session_id.clone();
        let on_msg_cb = Closure::wrap(Box::new(move |evt: web_sys::MessageEvent| {
            if let Some(text) = evt.data().as_string() {
                on_dc_message(sid_clone.clone(), text);
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        dc.set_onmessage(Some(on_msg_cb.as_ref().unchecked_ref()));
        on_msg_cb.forget();

        // Data channel onclose
        let ready_clone = ready.clone();
        let sid_clone = session_id.clone();
        let on_close_cb = Closure::wrap(Box::new(move |_: JsValue| {
            clog(&format!("[WEBRTC] Data channel closed for session {}", sid_clone));
            *ready_clone.lock().unwrap() = false;
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onclose(Some(on_close_cb.as_ref().unchecked_ref()));
        on_close_cb.forget();

        *dc_holder.lock().unwrap() = Some(dc);

        // ICE candidate handler — send candidates to peer via relay
        let ws_clone = ws.clone();
        let sid_clone = session_id.clone();
        let on_ice_cb = Closure::wrap(Box::new(move |evt: web_sys::RtcPeerConnectionIceEvent| {
            if let Some(candidate) = evt.candidate() {
                let candidate_json = serde_json::json!({
                    "candidate": candidate.candidate(),
                    "sdpMid": candidate.sdp_mid(),
                    "sdpMLineIndex": candidate.sdp_m_line_index(),
                });
                let msg = ClientMsg::Ice {
                    session_id: sid_clone.clone(),
                    candidate: candidate_json,
                };
                let json = serde_json::to_string(&msg).unwrap();
                let _ = ws_clone.send_with_str(&json);
            }
        }) as Box<dyn FnMut(web_sys::RtcPeerConnectionIceEvent)>);
        pc.set_onicecandidate(Some(on_ice_cb.as_ref().unchecked_ref()));
        on_ice_cb.forget();

        // Create offer and set local description, then send offer via relay
        let pc_clone = pc.clone();
        let ws_clone = ws.clone();
        let sid_clone = session_id.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let offer = match wasm_bindgen_futures::JsFuture::from(pc_clone.create_offer()).await {
                Ok(o) => o,
                Err(e) => {
                    clog(&format!("[WEBRTC] create_offer failed: {:?}", e));
                    return;
                }
            };

            let offer_sdp = js_sys::Reflect::get(&offer, &"sdp".into())
                .unwrap()
                .as_string()
                .unwrap();

            let desc = web_sys::RtcSessionDescriptionInit::new(web_sys::RtcSdpType::Offer);
            desc.set_sdp(&offer_sdp);

            if let Err(e) = wasm_bindgen_futures::JsFuture::from(
                pc_clone.set_local_description(&desc),
            ).await {
                clog(&format!("[WEBRTC] set_local_description (offer) failed: {:?}", e));
                return;
            }

            let sdp_json = serde_json::json!({
                "type": "offer",
                "sdp": offer_sdp,
            });
            let msg = ClientMsg::Sdp {
                session_id: sid_clone,
                sdp: sdp_json,
            };
            let json = serde_json::to_string(&msg).unwrap();
            let _ = ws_clone.send_with_str(&json);
            clog("[WEBRTC] Sent SDP offer via relay");
        });

        Ok(WebRtcSession {
            pc,
            dc: dc_holder,
            ready,
            local_audio_track: Arc::new(Mutex::new(None)),
            local_video_track: Arc::new(Mutex::new(None)),
            remote_stream,
            audio_sender: Arc::new(Mutex::new(None)),
            video_sender: Arc::new(Mutex::new(None)),
        })
    }

    /// Set up the answerer side: creates PeerConnection, applies the remote offer,
    /// creates an SDP answer, and sends it via the relay.
    pub fn setup_answerer(
        session_id: String,
        sdp_offer: &serde_json::Value,
        ws: &web_sys::WebSocket,
        on_dc_message: impl FnMut(String, String) + 'static,
        on_dc_open: impl FnMut(String) + 'static,
        on_remote_video: impl FnMut(String, bool) + 'static,
    ) -> Result<WebRtcSession, String> {
        use std::cell::RefCell;
        use std::rc::Rc;

        let pc = create_peer_connection()?;
        let dc_holder: Arc<Mutex<Option<web_sys::RtcDataChannel>>> = Arc::new(Mutex::new(None));
        let ready: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let remote_stream: Arc<Mutex<Option<web_sys::MediaStream>>> = Arc::new(Mutex::new(None));

        // ontrack — store remote media stream
        let remote_stream_clone = remote_stream.clone();
        let sid_for_track = session_id.clone();
        setup_ontrack_handler(&pc, remote_stream_clone, sid_for_track, on_remote_video);

        // Wrap callbacks in Rc<RefCell<>> so they can be shared between the ondatachannel
        // closure and the inner event handlers it creates.
        let on_dc_message = Rc::new(RefCell::new(on_dc_message));
        let on_dc_open = Rc::new(RefCell::new(Some(on_dc_open)));

        // Handle incoming data channel from offerer
        let dc_holder_clone = dc_holder.clone();
        let ready_clone = ready.clone();
        let sid_clone = session_id.clone();
        let on_datachannel_cb = Closure::wrap(Box::new(move |evt: web_sys::RtcDataChannelEvent| {
            let dc = evt.channel();
            clog(&format!("[WEBRTC] Received data channel '{}' for session {}", dc.label(), sid_clone));

            // onopen
            let ready_c = ready_clone.clone();
            let sid_c = sid_clone.clone();
            let on_dc_open_c = on_dc_open.clone();
            let open_cb = Closure::wrap(Box::new(move |_: JsValue| {
                clog(&format!("[WEBRTC] Answerer data channel open for session {}", sid_c));
                *ready_c.lock().unwrap() = true;
                if let Some(mut cb) = on_dc_open_c.borrow_mut().take() {
                    cb(sid_c.clone());
                }
            }) as Box<dyn FnMut(JsValue)>);
            dc.set_onopen(Some(open_cb.as_ref().unchecked_ref()));
            open_cb.forget();

            // onmessage
            let sid_c = sid_clone.clone();
            let on_dc_msg = on_dc_message.clone();
            let on_msg_cb = Closure::wrap(Box::new(move |evt: web_sys::MessageEvent| {
                if let Some(text) = evt.data().as_string() {
                    on_dc_msg.borrow_mut()(sid_c.clone(), text);
                }
            }) as Box<dyn FnMut(web_sys::MessageEvent)>);
            dc.set_onmessage(Some(on_msg_cb.as_ref().unchecked_ref()));
            on_msg_cb.forget();

            // onclose
            let ready_c = ready_clone.clone();
            let sid_c = sid_clone.clone();
            let on_close_cb = Closure::wrap(Box::new(move |_: JsValue| {
                clog(&format!("[WEBRTC] Answerer data channel closed for session {}", sid_c));
                *ready_c.lock().unwrap() = false;
            }) as Box<dyn FnMut(JsValue)>);
            dc.set_onclose(Some(on_close_cb.as_ref().unchecked_ref()));
            on_close_cb.forget();

            *dc_holder_clone.lock().unwrap() = Some(dc);
        }) as Box<dyn FnMut(web_sys::RtcDataChannelEvent)>);
        pc.set_ondatachannel(Some(on_datachannel_cb.as_ref().unchecked_ref()));
        on_datachannel_cb.forget();

        // ICE candidate handler
        let ws_clone = ws.clone();
        let sid_clone = session_id.clone();
        let on_ice_cb = Closure::wrap(Box::new(move |evt: web_sys::RtcPeerConnectionIceEvent| {
            if let Some(candidate) = evt.candidate() {
                let candidate_json = serde_json::json!({
                    "candidate": candidate.candidate(),
                    "sdpMid": candidate.sdp_mid(),
                    "sdpMLineIndex": candidate.sdp_m_line_index(),
                });
                let msg = ClientMsg::Ice {
                    session_id: sid_clone.clone(),
                    candidate: candidate_json,
                };
                let json = serde_json::to_string(&msg).unwrap();
                let _ = ws_clone.send_with_str(&json);
            }
        }) as Box<dyn FnMut(web_sys::RtcPeerConnectionIceEvent)>);
        pc.set_onicecandidate(Some(on_ice_cb.as_ref().unchecked_ref()));
        on_ice_cb.forget();

        // Set remote description (offer), create answer, send via relay
        let offer_sdp = sdp_offer.get("sdp").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let pc_clone = pc.clone();
        let ws_clone = ws.clone();
        let sid_clone = session_id.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let remote_desc = web_sys::RtcSessionDescriptionInit::new(web_sys::RtcSdpType::Offer);
            remote_desc.set_sdp(&offer_sdp);

            if let Err(e) = wasm_bindgen_futures::JsFuture::from(
                pc_clone.set_remote_description(&remote_desc),
            ).await {
                clog(&format!("[WEBRTC] set_remote_description (offer) failed: {:?}", e));
                return;
            }

            let answer = match wasm_bindgen_futures::JsFuture::from(pc_clone.create_answer()).await {
                Ok(a) => a,
                Err(e) => {
                    clog(&format!("[WEBRTC] create_answer failed: {:?}", e));
                    return;
                }
            };

            let answer_sdp = js_sys::Reflect::get(&answer, &"sdp".into())
                .unwrap()
                .as_string()
                .unwrap();

            let local_desc = web_sys::RtcSessionDescriptionInit::new(web_sys::RtcSdpType::Answer);
            local_desc.set_sdp(&answer_sdp);

            if let Err(e) = wasm_bindgen_futures::JsFuture::from(
                pc_clone.set_local_description(&local_desc),
            ).await {
                clog(&format!("[WEBRTC] set_local_description (answer) failed: {:?}", e));
                return;
            }

            let sdp_json = serde_json::json!({
                "type": "answer",
                "sdp": answer_sdp,
            });
            let msg = ClientMsg::Sdp {
                session_id: sid_clone,
                sdp: sdp_json,
            };
            let json = serde_json::to_string(&msg).unwrap();
            let _ = ws_clone.send_with_str(&json);
            clog("[WEBRTC] Sent SDP answer via relay");
        });

        Ok(WebRtcSession {
            pc,
            dc: dc_holder,
            ready,
            local_audio_track: Arc::new(Mutex::new(None)),
            local_video_track: Arc::new(Mutex::new(None)),
            remote_stream,
            audio_sender: Arc::new(Mutex::new(None)),
            video_sender: Arc::new(Mutex::new(None)),
        })
    }

    /// Offerer calls this when the SDP answer arrives from the answerer.
    pub fn handle_sdp_answer(pc: &web_sys::RtcPeerConnection, sdp_answer: &serde_json::Value) {
        let answer_sdp = sdp_answer.get("sdp").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let pc_clone = pc.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let desc = web_sys::RtcSessionDescriptionInit::new(web_sys::RtcSdpType::Answer);
            desc.set_sdp(&answer_sdp);
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(
                pc_clone.set_remote_description(&desc),
            ).await {
                clog(&format!("[WEBRTC] set_remote_description (answer) failed: {:?}", e));
            } else {
                clog("[WEBRTC] Set remote description (answer) successfully");
            }
        });
    }

    /// Add a trickle ICE candidate received from the remote peer.
    pub fn handle_ice_candidate(pc: &web_sys::RtcPeerConnection, candidate: &serde_json::Value) {
        let candidate_str = candidate.get("candidate").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sdp_mid = candidate.get("sdpMid").and_then(|v| v.as_str()).map(|s| s.to_string());
        let sdp_m_line_index = candidate.get("sdpMLineIndex").and_then(|v| v.as_u64()).map(|n| n as u16);

        let init = web_sys::RtcIceCandidateInit::new(&candidate_str);
        if let Some(ref mid) = sdp_mid {
            init.set_sdp_mid(Some(mid));
        }
        if let Some(idx) = sdp_m_line_index {
            init.set_sdp_m_line_index(Some(idx));
        }

        let pc_clone = pc.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match web_sys::RtcIceCandidate::new(&init) {
                Ok(rtc_candidate) => {
                    if let Err(e) = wasm_bindgen_futures::JsFuture::from(
                        pc_clone.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&rtc_candidate)),
                    ).await {
                        clog(&format!("[WEBRTC] add_ice_candidate failed: {:?}", e));
                    }
                }
                Err(e) => {
                    clog(&format!("[WEBRTC] RtcIceCandidate::new failed: {:?}", e));
                }
            }
        });
    }

    // ---------- Media helpers ----------

    /// Register the ontrack handler on a PeerConnection. Stores the remote
    /// MediaStream and attaches it to the DOM video element for the session.
    /// `on_remote_video(session_id, has_video)` is called when video tracks
    /// arrive or end, allowing the UI to update reactively.
    fn setup_ontrack_handler(
        pc: &web_sys::RtcPeerConnection,
        remote_stream: Arc<Mutex<Option<web_sys::MediaStream>>>,
        session_id: String,
        on_remote_video: impl FnMut(String, bool) + 'static,
    ) {
        use wasm_bindgen::JsCast;
        use std::rc::Rc;
        use std::cell::RefCell;

        let on_remote_video = Rc::new(RefCell::new(on_remote_video));

        let on_track_cb = Closure::wrap(Box::new(move |evt: web_sys::RtcTrackEvent| {
            let streams = evt.streams();
            if streams.length() > 0 {
                let stream: web_sys::MediaStream = streams.get(0).unchecked_into();
                let track = evt.track();
                let is_video = track.kind() == "video";

                clog(&format!("[WEBRTC] ontrack: {} track for session {}", track.kind(), session_id));
                *remote_stream.lock().unwrap() = Some(stream.clone());

                if is_video {
                    attach_remote_stream_to_video(&session_id, &stream);
                    on_remote_video.borrow_mut()(session_id.clone(), true);

                    // Listen for track ended/mute to detect when remote camera stops
                    let sid = session_id.clone();
                    let cb = on_remote_video.clone();
                    let on_ended = Closure::wrap(Box::new(move |_: JsValue| {
                        clog(&format!("[WEBRTC] Remote video track ended for session {}", sid));
                        detach_remote_video(&sid);
                        cb.borrow_mut()(sid.clone(), false);
                    }) as Box<dyn FnMut(JsValue)>);
                    track.set_onended(Some(on_ended.as_ref().unchecked_ref()));
                    on_ended.forget();
                }
            }
        }) as Box<dyn FnMut(web_sys::RtcTrackEvent)>);
        pc.set_ontrack(Some(on_track_cb.as_ref().unchecked_ref()));
        on_track_cb.forget();
    }

    /// Set srcObject on the Dioxus-rendered <video> element for this session.
    /// The element must already exist in the DOM (rendered by RSX when TV is on).
    /// If TV is off (element absent), the stream is still stored in remote_stream
    /// and will be attached when TV toggles on via the onmounted callback.
    pub fn attach_remote_stream_to_video(session_id: &str, stream: &web_sys::MediaStream) {
        let doc = match web_sys::window().and_then(|w| w.document()) {
            Some(d) => d,
            None => return,
        };
        let el_id = format!("remote-video-el-{}", session_id);
        match doc.get_element_by_id(&el_id) {
            Some(el) => {
                let _ = js_sys::Reflect::set(&el, &"srcObject".into(), stream);
                clog(&format!("[WEBRTC] Attached remote stream to #{}", el_id));
            }
            None => {
                clog(&format!("[WEBRTC] #{} not in DOM (TV off?), stream stored for later", el_id));
            }
        }
    }

    /// Clear srcObject on the video element when the remote peer stops their camera.
    pub fn detach_remote_video(session_id: &str) {
        let doc = match web_sys::window().and_then(|w| w.document()) {
            Some(d) => d,
            None => return,
        };
        let el_id = format!("remote-video-el-{}", session_id);
        if let Some(el) = doc.get_element_by_id(&el_id) {
            let _ = js_sys::Reflect::set(&el, &"srcObject".into(), &wasm_bindgen::JsValue::NULL);
            clog(&format!("[WEBRTC] Cleared srcObject on #{}", el_id));
        }
    }

    /// Create a new SDP offer on the existing PeerConnection and send it
    /// through the relay, triggering renegotiation.
    fn renegotiate(pc: &web_sys::RtcPeerConnection, ws: &web_sys::WebSocket, session_id: &str) {
        let pc = pc.clone();
        let ws = ws.clone();
        let sid = session_id.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            let offer = match wasm_bindgen_futures::JsFuture::from(pc.create_offer()).await {
                Ok(o) => o,
                Err(e) => {
                    clog(&format!("[WEBRTC] renegotiate create_offer failed: {:?}", e));
                    return;
                }
            };
            let offer_sdp = js_sys::Reflect::get(&offer, &"sdp".into())
                .unwrap()
                .as_string()
                .unwrap();
            let desc = web_sys::RtcSessionDescriptionInit::new(web_sys::RtcSdpType::Offer);
            desc.set_sdp(&offer_sdp);
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(pc.set_local_description(&desc)).await {
                clog(&format!("[WEBRTC] renegotiate setLocalDescription failed: {:?}", e));
                return;
            }
            let sdp_json = serde_json::json!({ "type": "offer", "sdp": offer_sdp });
            let msg = ClientMsg::Sdp { session_id: sid, sdp: sdp_json };
            let json = serde_json::to_string(&msg).unwrap();
            let _ = ws.send_with_str(&json);
            clog("[WEBRTC] Sent renegotiation offer");
        });
    }

    /// Handle a renegotiation offer on an existing PeerConnection. This applies
    /// the remote offer and sends back an answer, without creating a new PC.
    pub fn handle_renegotiation_offer(
        pc: &web_sys::RtcPeerConnection,
        sdp: &serde_json::Value,
        ws: &web_sys::WebSocket,
        session_id: &str,
    ) {
        let offer_sdp = sdp.get("sdp").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let pc = pc.clone();
        let ws = ws.clone();
        let sid = session_id.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            let remote_desc = web_sys::RtcSessionDescriptionInit::new(web_sys::RtcSdpType::Offer);
            remote_desc.set_sdp(&offer_sdp);
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(pc.set_remote_description(&remote_desc)).await {
                clog(&format!("[WEBRTC] renegotiation setRemoteDescription failed: {:?}", e));
                return;
            }
            let answer = match wasm_bindgen_futures::JsFuture::from(pc.create_answer()).await {
                Ok(a) => a,
                Err(e) => {
                    clog(&format!("[WEBRTC] renegotiation create_answer failed: {:?}", e));
                    return;
                }
            };
            let answer_sdp = js_sys::Reflect::get(&answer, &"sdp".into())
                .unwrap()
                .as_string()
                .unwrap();
            let local_desc = web_sys::RtcSessionDescriptionInit::new(web_sys::RtcSdpType::Answer);
            local_desc.set_sdp(&answer_sdp);
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(pc.set_local_description(&local_desc)).await {
                clog(&format!("[WEBRTC] renegotiation setLocalDescription failed: {:?}", e));
                return;
            }
            let sdp_json = serde_json::json!({ "type": "answer", "sdp": answer_sdp });
            let msg = ClientMsg::Sdp { session_id: sid, sdp: sdp_json };
            let json = serde_json::to_string(&msg).unwrap();
            let _ = ws.send_with_str(&json);
            clog("[WEBRTC] Sent renegotiation answer");
        });
    }

    /// Start capturing audio from the microphone. Adds the track to the
    /// PeerConnection and triggers renegotiation.
    pub fn start_mic(session: &WebRtcSession, ws: &web_sys::WebSocket, session_id: &str) {
        use wasm_bindgen::JsCast;

        let window = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        let navigator = window.navigator();
        let media_devices = match navigator.media_devices() {
            Ok(md) => md,
            Err(e) => {
                clog(&format!("[WEBRTC] media_devices() failed: {:?}", e));
                return;
            }
        };

        let constraints = web_sys::MediaStreamConstraints::new();
        constraints.set_audio(&true.into());
        constraints.set_video(&false.into());

        let promise = match media_devices.get_user_media_with_constraints(&constraints) {
            Ok(p) => p,
            Err(e) => {
                clog(&format!("[WEBRTC] getUserMedia(audio) failed: {:?}", e));
                return;
            }
        };

        let pc = session.pc.clone();
        let ws = ws.clone();
        let sid = session_id.to_string();
        let local_audio = session.local_audio_track.clone();
        let audio_sender = session.audio_sender.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let stream_js = match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(s) => s,
                Err(e) => {
                    clog(&format!("[WEBRTC] getUserMedia(audio) promise rejected: {:?}", e));
                    return;
                }
            };
            let stream: web_sys::MediaStream = stream_js.unchecked_into();
            let tracks = stream.get_audio_tracks();
            if tracks.length() == 0 {
                clog("[WEBRTC] No audio tracks in stream");
                return;
            }
            let track: web_sys::MediaStreamTrack = tracks.get(0).unchecked_into();
            let sender = pc.add_track_0(&track, &stream);
            *local_audio.lock().unwrap() = Some(track);
            *audio_sender.lock().unwrap() = Some(sender);
            clog("[WEBRTC] Added local audio track, renegotiating");
            renegotiate(&pc, &ws, &sid);
        });
    }

    /// Stop the microphone. Removes the track from the PeerConnection and
    /// triggers renegotiation.
    pub fn stop_mic(session: &WebRtcSession, ws: &web_sys::WebSocket, session_id: &str) {
        if let Some(track) = session.local_audio_track.lock().unwrap().take() {
            track.stop();
        }
        if let Some(sender) = session.audio_sender.lock().unwrap().take() {
            let _ = session.pc.remove_track(&sender);
            clog("[WEBRTC] Removed local audio track, renegotiating");
            renegotiate(&session.pc, ws, session_id);
        }
    }

    /// Start capturing video from the camera. Adds the track to the
    /// PeerConnection and triggers renegotiation.
    pub fn start_camera(session: &WebRtcSession, ws: &web_sys::WebSocket, session_id: &str) {
        use wasm_bindgen::JsCast;

        let window = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        let navigator = window.navigator();
        let media_devices = match navigator.media_devices() {
            Ok(md) => md,
            Err(e) => {
                clog(&format!("[WEBRTC] media_devices() failed: {:?}", e));
                return;
            }
        };

        let constraints = web_sys::MediaStreamConstraints::new();
        constraints.set_audio(&false.into());
        constraints.set_video(&true.into());

        let promise = match media_devices.get_user_media_with_constraints(&constraints) {
            Ok(p) => p,
            Err(e) => {
                clog(&format!("[WEBRTC] getUserMedia(video) failed: {:?}", e));
                return;
            }
        };

        let pc = session.pc.clone();
        let ws = ws.clone();
        let sid = session_id.to_string();
        let local_video = session.local_video_track.clone();
        let video_sender = session.video_sender.clone();
        let dc = session.dc.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let stream_js = match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(s) => s,
                Err(e) => {
                    clog(&format!("[WEBRTC] getUserMedia(video) promise rejected: {:?}", e));
                    return;
                }
            };
            let stream: web_sys::MediaStream = stream_js.unchecked_into();
            let tracks = stream.get_video_tracks();
            if tracks.length() == 0 {
                clog("[WEBRTC] No video tracks in stream");
                return;
            }
            let track: web_sys::MediaStreamTrack = tracks.get(0).unchecked_into();
            let sender = pc.add_track_0(&track, &stream);
            *local_video.lock().unwrap() = Some(track);
            *video_sender.lock().unwrap() = Some(sender);
            // Notify peer that camera is on
            if let Some(ref dc) = *dc.lock().unwrap() {
                let _ = dc.send_with_str(&format!("{}camera_on", DC_CONTROL_PREFIX));
            }
            clog("[WEBRTC] Added local video track, renegotiating");
            renegotiate(&pc, &ws, &sid);
        });
    }

    /// Stop the camera. Removes the track from the PeerConnection and
    /// triggers renegotiation.
    pub fn stop_camera(session: &WebRtcSession, ws: &web_sys::WebSocket, session_id: &str) {
        if let Some(track) = session.local_video_track.lock().unwrap().take() {
            track.stop();
        }
        // Notify peer that camera is off
        if let Some(ref dc) = *session.dc.lock().unwrap() {
            let _ = dc.send_with_str(&format!("{}camera_off", DC_CONTROL_PREFIX));
        }
        if let Some(sender) = session.video_sender.lock().unwrap().take() {
            let _ = session.pc.remove_track(&sender);
            clog("[WEBRTC] Removed local video track, renegotiating");
            renegotiate(&session.pc, ws, session_id);
        }
    }

    /// Mute or unmute remote audio (no renegotiation needed).
    /// Toggles both the remote audio track enabled state and the <video>
    /// element's muted attribute (needed because we start muted for autoplay).
    pub fn set_remote_audio_enabled(session: &WebRtcSession, enabled: bool, session_id: &str) {
        use wasm_bindgen::JsCast;

        if let Some(ref stream) = *session.remote_stream.lock().unwrap() {
            let tracks = stream.get_audio_tracks();
            for i in 0..tracks.length() {
                let track: web_sys::MediaStreamTrack = tracks.get(i).unchecked_into();
                track.set_enabled(enabled);
            }
        }

        // Also toggle the video element's muted state
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let el_id = format!("remote-video-el-{}", session_id);
            if let Some(el) = doc.get_element_by_id(&el_id) {
                let _ = js_sys::Reflect::set(&el, &"muted".into(), &(!enabled).into());
            }
        }

        clog(&format!("[WEBRTC] Remote audio enabled={}", enabled));
    }

    /// Close a WebRTC session and clean up resources.
    pub fn close_session(session: &WebRtcSession) {
        // Stop local media tracks to release hardware
        if let Some(track) = session.local_audio_track.lock().unwrap().take() {
            track.stop();
        }
        if let Some(track) = session.local_video_track.lock().unwrap().take() {
            track.stop();
        }
        if let Some(ref dc) = *session.dc.lock().unwrap() {
            dc.close();
        }
        session.pc.close();
        *session.ready.lock().unwrap() = false;
    }
}

// Non-WASM stubs for type checking
#[cfg(not(target_family = "wasm"))]
pub mod wasm {
    use super::*;

    #[allow(dead_code)]
    pub fn connect(
        _relay_url: &str,
        _pubkey_hex: &str,
        _signing_key_bytes: [u8; 32],
        _on_message: impl Fn(ServerMessage) + 'static,
        _on_open: impl Fn() + 'static,
        _on_close: impl Fn() + 'static,
    ) -> Result<(), String> {
        Ok(())
    }

    #[allow(dead_code)]
    pub fn send_msg(_ws: &(), _msg: &ClientMsg) -> Result<(), String> {
        Ok(())
    }
}
