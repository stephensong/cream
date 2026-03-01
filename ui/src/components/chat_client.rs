use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    Invite { from: String, session_id: String, ecdh_pubkey: String },
    Accept { session_id: String, ecdh_pubkey: String },
    Decline { session_id: String },
    Text { session_id: String, ciphertext: String, nonce: String },
    Sdp { session_id: String, sdp: serde_json::Value },
    Ice { session_id: String, candidate: serde_json::Value },
    Close { session_id: String, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)] // used in WASM relay communication
pub enum ClientMsg {
    Auth { public_key: String, signature: String, nonce: String },
    Invite { to: String, session_id: String, ecdh_pubkey: String },
    Accept { session_id: String, ecdh_pubkey: String },
    Decline { session_id: String },
    Text { session_id: String, ciphertext: String, nonce: String },
    Sdp { session_id: String, sdp: serde_json::Value },
    Ice { session_id: String, candidate: serde_json::Value },
    Close { session_id: String },
}

// ---------- Chat session ----------

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub sender_is_me: bool,
    pub body: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields used in WASM UI
pub struct ChatSession {
    pub session_id: String,
    pub peer_pubkey: String,
    pub messages: Vec<ChatMessage>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub deposit_paid: u64,
    pub has_av: bool,
}

// ---------- Chat state (shared via Signal in UI) ----------

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // fields used in WASM UI
pub struct ChatState {
    pub connected: bool,
    pub authenticated: bool,
    pub sessions: HashMap<String, ChatSession>,
    pub pending_invites: Vec<PendingInvite>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PendingInvite {
    pub from: String,
    pub session_id: String,
    pub ecdh_pubkey: String,
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
        on_message: impl Fn(ServerMessage) + 'static,
        on_open: impl Fn() + 'static,
        on_close: impl Fn() + 'static,
    ) -> Result<WebSocket, String> {
        let ws = WebSocket::new(relay_url)
            .map_err(|e| format!("WebSocket connect failed: {:?}", e))?;

        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let pubkey = pubkey_hex.to_string();

        // onopen
        let ws_clone = ws.clone();
        let on_open_cb = Closure::wrap(Box::new(move |_: JsValue| {
            clog("[CHAT] WebSocket connected, waiting for nonce...");
            let _ = &ws_clone; // keep alive
            on_open();
        }) as Box<dyn Fn(JsValue)>);
        ws.set_onopen(Some(on_open_cb.as_ref().unchecked_ref()));
        on_open_cb.forget();

        // onmessage — handle auth handshake and forward messages
        let ws_clone = ws.clone();
        let pubkey_clone = pubkey.clone();
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
        }) as Box<dyn Fn(web_sys::MessageEvent)>);
        ws.set_onmessage(Some(on_msg_cb.as_ref().unchecked_ref()));
        on_msg_cb.forget();

        // onclose
        let on_close_cb = Closure::wrap(Box::new(move |_: JsValue| {
            clog("[CHAT] WebSocket closed");
            on_close();
        }) as Box<dyn Fn(JsValue)>);
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

    /// Create an RTC peer connection with default STUN config.
    pub fn create_peer_connection() -> Result<web_sys::RtcPeerConnection, String> {
        let ice_servers = js_sys::Array::new();
        let stun = js_sys::Object::new();
        js_sys::Reflect::set(
            &stun,
            &"urls".into(),
            &"stun:stun.l.google.com:19302".into(),
        )
        .map_err(|e| format!("Reflect error: {:?}", e))?;
        ice_servers.push(&stun);

        let config = web_sys::RtcConfiguration::new();
        config.set_ice_servers(&ice_servers);

        web_sys::RtcPeerConnection::new_with_configuration(&config)
            .map_err(|e| format!("RtcPeerConnection failed: {:?}", e))
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
