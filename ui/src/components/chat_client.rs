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

// ---------- WASM WebSocket client ----------

#[cfg(target_family = "wasm")]
pub mod wasm {
    use super::*;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::WebSocket;

    fn clog(msg: &str) {
        web_sys::console::log_1(&msg.into());
    }

    /// Connect to the relay, authenticate, and return the WebSocket.
    /// Incoming messages are dispatched to `on_message`.
    pub fn connect(
        relay_url: &str,
        pubkey_hex: &str,
        on_message: impl Fn(ServerMessage) + 'static,
        on_open: impl Fn() + 'static,
        on_close: impl Fn() + 'static,
    ) -> Result<WebSocket, String> {
        let ws = WebSocket::new(relay_url)
            .map_err(|e| format!("WebSocket connect failed: {:?}", e))?;

        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        // Store pubkey for auth handshake
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

            // Auto-handle nonce → sign and authenticate
            if let ServerMessage::Nonce { ref nonce } = msg {
                clog(&format!("[CHAT] Got nonce, signing with key {}..{}", &pubkey_clone[..8], &pubkey_clone[pubkey_clone.len()-8..]));
                let nonce = nonce.clone();
                let pubkey = pubkey_clone.clone();
                let ws = ws_clone.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match sign_nonce_with_delegate(&nonce).await {
                        Ok(signature) => {
                            let auth = ClientMsg::Auth {
                                public_key: pubkey,
                                signature,
                                nonce,
                            };
                            let json = serde_json::to_string(&auth).unwrap();
                            let _ = ws.send_with_str(&json);
                        }
                        Err(e) => {
                            clog(&format!("[CHAT] Auth signing failed: {}", e));
                        }
                    }
                });
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

    /// Sign a nonce using the delegate's private key from localStorage.
    /// This mirrors how the signing_service signs messages.
    async fn sign_nonce_with_delegate(nonce: &str) -> Result<String, String> {
        use ed25519_dalek::{Signer, SigningKey};

        let window = web_sys::window().ok_or("No window")?;
        let storage = window
            .local_storage()
            .map_err(|_| "No localStorage")?
            .ok_or("localStorage unavailable")?;

        let key_hex = storage
            .get_item("cream_private_key")
            .map_err(|_| "Failed to read key")?
            .ok_or("No private key in localStorage")?;

        let key_bytes = hex_to_bytes(&key_hex)?;
        let signing_key = SigningKey::from_bytes(
            &key_bytes.try_into().map_err(|_| "Invalid key length")?,
        );

        let signature = signing_key.sign(nonce.as_bytes());
        Ok(bytes_to_hex(&signature.to_bytes()))
    }

    fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
        (0..hex.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&hex[i..i + 2], 16)
                    .map_err(|e| format!("Invalid hex: {}", e))
            })
            .collect()
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
