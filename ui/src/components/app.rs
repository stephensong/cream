use dioxus::prelude::*;
use dioxus_router::Navigator;

use cream_common::postcode::{
    is_valid_postcode, lookup_all_localities, lookup_postcode, PostcodeInfo,
};

use super::directory_view::DirectoryView;
use super::faq_view::FaqView;
use super::guardian_admin::GuardianAdmin;
use super::iaq_view::IaqView;
use super::key_manager::KeyManager;
use super::market_dashboard::MarketDashboard;
use super::market_view::MarketView;
use super::markets_list_view::MarketsListView;
use super::my_orders::MyOrders;
use super::node_api::{use_node_action, use_node_coroutine, NodeAction};
use super::shared_state::{use_shared_state, SharedState};
use super::messages_view::MessagesView;
use super::storefront_view::StorefrontView;
use super::supplier_dashboard::SupplierDashboard;
use super::user_state::{use_user_state, UserState};
use super::wallet_view::WalletView;
#[allow(unused_imports)] // SessionStatus used in WASM cfg block
use super::chat_client::{ChatState, ChatWsHandle, SessionStatus};
#[cfg(target_family = "wasm")]
use super::chat_client::WebRtcSessions;
use super::chat_view::{ChatPanel, ChatInviteBanner};
use super::profile_view::ProfileView;

/// Handle a data-channel control message (prefixed with `__ctrl:`).
#[cfg(target_family = "wasm")]
fn handle_dc_control(chat: &mut Signal<ChatState>, session_id: &str, ctrl: &str) {
    use super::chat_client::PaymentRequest;

    if ctrl == "camera_on" {
        if let Some(s) = chat.write().sessions.get_mut(session_id) {
            s.has_remote_video = true;
        }
        web_sys::console::log_1(&format!("[WEBRTC] Peer camera ON for {}", session_id).into());
    } else if ctrl == "camera_off" {
        if let Some(s) = chat.write().sessions.get_mut(session_id) {
            s.has_remote_video = false;
        }
        super::chat_client::wasm::detach_remote_video(session_id);
        web_sys::console::log_1(&format!("[WEBRTC] Peer camera OFF for {}", session_id).into());
    } else if let Some(amount_str) = ctrl.strip_prefix("pay_request:") {
        if let Ok(amount) = amount_str.parse::<u64>() {
            if let Some(s) = chat.write().sessions.get_mut(session_id) {
                s.payment_request = Some(PaymentRequest::ReceivedPending { curd_per_interval: amount });
            }
            web_sys::console::log_1(&format!("[WEBRTC] Peer requests {} CURD/interval for {}", amount, session_id).into());
        }
    } else if ctrl == "pay_accept" {
        let mut state = chat.write();
        if let Some(s) = state.sessions.get_mut(session_id) {
            if let Some(PaymentRequest::SentPending { curd_per_interval }) = s.payment_request {
                s.payment_request = Some(PaymentRequest::ActiveReceiving { curd_per_interval });
            }
        }
        web_sys::console::log_1(&format!("[WEBRTC] Peer accepted payment for {}", session_id).into());
    } else if ctrl == "pay_decline" {
        if let Some(s) = chat.write().sessions.get_mut(session_id) {
            s.payment_request = None;
        }
        web_sys::console::log_1(&format!("[WEBRTC] Peer declined payment for {}", session_id).into());
    } else if ctrl == "pay_stop" {
        if let Some(s) = chat.write().sessions.get_mut(session_id) {
            s.payment_request = None;
        }
        web_sys::console::log_1(&format!("[WEBRTC] Peer stopped payment for {}", session_id).into());
    } else {
        web_sys::console::log_1(&format!("[WEBRTC] Unknown control: {}", ctrl).into());
    }
}

/// Read `?supplier=X` from the browser URL bar. Returns `None` outside WASM.
fn get_supplier_query_param() -> Option<String> {
    #[cfg(target_family = "wasm")]
    {
        let search = web_sys::window()?.location().search().ok()?;
        let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
        params.get("supplier").filter(|s| !s.is_empty())
    }
    #[cfg(not(target_family = "wasm"))]
    {
        None
    }
}

/// Normalize a name to title case: "gary" → "Gary", "GARY" → "Gary".
fn title_case(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap().to_uppercase().to_string();
    first + &chars.as_str().to_lowercase()
}

#[derive(Clone, Debug, PartialEq, Routable)]
pub enum Route {
    #[layout(AppLayout)]
    #[route("/directory")]
    Directory {},
    #[route("/markets")]
    Markets {},
    #[route("/supplier/:name")]
    Supplier { name: String },
    #[route("/orders")]
    Orders {},
    #[route("/messages")]
    Messages {},
    #[route("/my_storefront")]
    Dashboard {},
    #[route("/market/:market_organizer")]
    Market { market_organizer: String },
    #[route("/my_market")]
    MyMarket {},
    #[route("/wallet")]
    Wallet {},
    #[route("/faq")]
    Faq {},
    #[route("/iaq")]
    Iaq {},
    #[route("/guardian")]
    Guardian {},
    #[route("/profile")]
    Profile {},
    #[redirect("/", || Route::Directory {})]
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

#[component]
pub fn App() -> Element {
    use_context_provider(|| Signal::new(UserState::new()));
    use_context_provider(|| Signal::new(SharedState::new()));
    // Try to auto-derive KeyManager from sessionStorage credentials
    use_context_provider(|| {
        let km = auto_derive_key_manager();
        Signal::new(km)
    });
    use_context_provider(|| Signal::new(cream_common::tolls::TollRates::default()));
    use_context_provider(|| Signal::new(super::toll_rates::AdminStatus::default())); // is_admin
    use_context_provider(|| Signal::new(ChatState::default()));
    use_context_provider(|| Signal::new(ChatWsHandle::default()));
    use_context_provider(|| Signal::new(None::<String>)); // Pre-fill inbox recipient
    #[cfg(target_family = "wasm")]
    use_context_provider(|| Signal::new(WebRtcSessions::default()));
    use_node_coroutine();

    // Derive toll rates reactively from root user contract (no polling needed)
    {
        let shared: Signal<SharedState> = use_shared_state();
        let mut toll_signal: Signal<cream_common::tolls::TollRates> = use_context();
        use_effect(move || {
            if let Some(root) = shared.read().root_user_contract.as_ref() {
                toll_signal.set(root.toll_rates.clone());
            }
        });
    }

    // Check admin status reactively when key_manager changes (e.g. on login)
    {
        let key_manager: Signal<Option<KeyManager>> = use_context();
        let mut is_admin: Signal<super::toll_rates::AdminStatus> = use_context();
        use_effect(move || {
            // Synchronous read creates reactive dependency on key_manager signal
            let km_opt = key_manager.read().clone();
            if let Some(km) = km_opt {
                let pubkey_hex = km.pubkey_hex();
                spawn(async move {
                    let status = super::toll_rates::check_admin_status(&pubkey_hex).await;
                    is_admin.set(status);
                });
            }
        });
    }

    let key_manager: Signal<Option<KeyManager>> = use_context();
    let user_state = use_user_state();

    let setup_needed = key_manager.read().is_none() || user_state.read().moniker.is_none();

    let content = if setup_needed {
        rsx! { SetupScreen {} }
    } else {
        rsx! { Router::<Route> {} }
    };

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }
        {content}
    }
}

/// Returns "nav-active" if the current route matches, empty string otherwise.
fn nav_class(current: &Route, target: &Route) -> &'static str {
    let matches = match (current, target) {
        (Route::Supplier { .. }, Route::Supplier { .. }) => true,
        (Route::Market { .. }, Route::Market { .. }) => true,
        _ => std::mem::discriminant(current) == std::mem::discriminant(target),
    };
    if matches { "nav-active" } else { "" }
}

/// Render the navigation buttons for the app header.
fn nav_buttons(nav: Navigator, order_count: usize, displayed_balance: u64, is_supplier: bool, connected_supplier: Option<String>, inbox_count: usize, admin_status: super::toll_rates::AdminStatus) -> Element {
    let current_route = use_route::<Route>();
    if let Some(supplier) = connected_supplier {
        // Customer mode: single-storefront nav
        rsx! {
            nav {
                {
                    let supplier_clone = supplier.clone();
                    let cls = nav_class(&current_route, &Route::Supplier { name: String::new() });
                    rsx! {
                        button {
                            class: cls,
                            onclick: move |_| { nav.push(Route::Supplier { name: supplier_clone.clone() }); },
                            "Storefront"
                        }
                    }
                }
                button {
                    class: nav_class(&current_route, &Route::Orders {}),
                    onclick: move |_| { nav.push(Route::Orders {}); },
                    "My Orders ({order_count})"
                }
                button {
                    class: nav_class(&current_route, &Route::Messages {}),
                    onclick: move |_| { nav.push(Route::Messages {}); },
                    if inbox_count > 0 { "Inbox ({inbox_count})" } else { "Inbox" }
                }
                button {
                    class: nav_class(&current_route, &Route::Wallet {}),
                    onclick: move |_| { nav.push(Route::Wallet {}); },
                    "Wallet ({displayed_balance} CURD)"
                }
                if admin_status.admin {
                    button {
                        class: nav_class(&current_route, &Route::Guardian {}),
                        onclick: move |_| { nav.push(Route::Guardian {}); },
                        if admin_status.root { "Root" } else { "Admin" }
                    }
                }
            }
        }
    } else {
        // Supplier / browser mode: full directory nav
        rsx! {
            nav {
                button {
                    class: nav_class(&current_route, &Route::Directory {}),
                    onclick: move |_| { nav.push(Route::Directory {}); },
                    "Suppliers"
                }
                button {
                    class: nav_class(&current_route, &Route::Markets {}),
                    onclick: move |_| { nav.push(Route::Markets {}); },
                    "Markets"
                }
                button {
                    class: nav_class(&current_route, &Route::Orders {}),
                    onclick: move |_| { nav.push(Route::Orders {}); },
                    "My Orders ({order_count})"
                }
                button {
                    class: nav_class(&current_route, &Route::Messages {}),
                    onclick: move |_| { nav.push(Route::Messages {}); },
                    if inbox_count > 0 { "Inbox ({inbox_count})" } else { "Inbox" }
                }
                if is_supplier {
                    button {
                        class: nav_class(&current_route, &Route::Dashboard {}),
                        onclick: move |_| { nav.push(Route::Dashboard {}); },
                        "My Storefront"
                    }
                }
                button {
                    class: nav_class(&current_route, &Route::MyMarket {}),
                    onclick: move |_| { nav.push(Route::MyMarket {}); },
                    "My Markets"
                }
                button {
                    class: nav_class(&current_route, &Route::Wallet {}),
                    onclick: move |_| { nav.push(Route::Wallet {}); },
                    "Wallet ({displayed_balance} CURD)"
                }
                if admin_status.admin {
                    button {
                        class: nav_class(&current_route, &Route::Guardian {}),
                        onclick: move |_| { nav.push(Route::Guardian {}); },
                        if admin_status.root { "Root" } else { "Admin" }
                    }
                }
            }
        }
    }
}

/// Try to derive KeyManager from credentials stored in sessionStorage.
/// Returns Some(km) if moniker + password are available, None otherwise.
fn auto_derive_key_manager() -> Option<KeyManager> {
    let user_state = UserState::new();
    let _moniker = user_state.moniker.as_deref()?;
    if user_state.is_root {
        return Some(KeyManager::for_root());
    }
    let password = UserState::load_password()?;
    KeyManager::from_credentials(_moniker, &password).ok()
}

/// Manages the WebSocket connection to the chat relay.
/// Connects when KeyManager is available, routes incoming messages to ChatState.
fn use_chat_connection() {
    let key_manager: Signal<Option<KeyManager>> = use_context();
    #[allow(unused_variables, unused_mut)] // used in WASM cfg block
    let mut chat_state: Signal<ChatState> = use_context();
    #[allow(unused_mut)] // mutated in WASM cfg block
    let mut ws_handle: Signal<ChatWsHandle> = use_context();

    use_effect(move || {
        // Only connect if we have keys and aren't already connected
        #[allow(unused_variables)] // used in WASM cfg block
        let km = match key_manager.read().as_ref() {
            Some(km) => km.clone(),
            None => return,
        };

        if ws_handle.read().is_connected() {
            return;
        }

        #[cfg(target_family = "wasm")]
        let pubkey_hex = km.pubkey_hex();
        #[cfg(target_family = "wasm")]
        let signing_key_bytes = km.signing_key_bytes();
        #[cfg(target_family = "wasm")]
        let relay = super::chat_client::relay_url();

        #[cfg(target_family = "wasm")]
        {
            use super::chat_client::{ServerMessage, ChatMessage, ChatSession};
            use super::shared_state::SharedState;

            let shared_for_msg: Signal<SharedState> = use_context();
            let mut chat_for_msg = chat_state;
            let mut chat_for_open = chat_state;
            let mut chat_for_close = chat_state;
            let mut ws_for_close = ws_handle;
            let ws_for_webrtc = ws_handle;
            let mut webrtc_for_msg: Signal<WebRtcSessions> = use_context();
            let mut webrtc_for_close: Signal<WebRtcSessions> = use_context();

            match super::chat_client::wasm::connect(
                &relay,
                &pubkey_hex,
                signing_key_bytes,
                move |msg| {
                    match msg {
                        ServerMessage::AuthOk => {
                            chat_for_msg.write().authenticated = true;
                        }
                        ServerMessage::Error { ref message } => {
                            // If peer not connected, remove any PendingAccept session
                            if message.contains("Peer not connected") {
                                let mut state = chat_for_msg.write();
                                state.sessions.retain(|_, s| s.status != SessionStatus::PendingAccept);
                                state.last_error = Some(message.clone());
                            } else {
                                chat_for_msg.write().last_error = Some(message.clone());
                            }
                        }
                        ServerMessage::Invite { from, session_id, message, .. } => {
                            // Resolve peer name from directory
                            let peer_name = {
                                let shared_read = shared_for_msg.read();
                                let mut name = None;
                                for entry in shared_read.directory.entries.values() {
                                    let bytes = entry.supplier.0.to_bytes();
                                    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                                    if hex == from {
                                        name = Some(entry.name.clone());
                                        break;
                                    }
                                }
                                name.unwrap_or_else(|| {
                                    if from.len() > 16 {
                                        format!("{}...{}", &from[..8], &from[from.len()-8..])
                                    } else {
                                        from.clone()
                                    }
                                })
                            };
                            let invite_msg = ChatMessage {
                                sender_is_me: false,
                                sender_name: peer_name.clone(),
                                body: message,
                                timestamp: chrono::Utc::now(),
                            };
                            let session = ChatSession {
                                session_id: session_id.clone(),
                                peer_pubkey: from,
                                peer_name,
                                messages: vec![invite_msg],
                                started_at: chrono::Utc::now(),
                                status: SessionStatus::InviteReceived,
                                mic_enabled: false,
                                speaker_enabled: false,
                                camera_enabled: false,
                                tv_enabled: false,
                                has_remote_video: false,
                                is_initiator: false,
                                payment_request: None,
                            };
                            chat_for_msg.write().sessions.insert(session_id, session);
                        }
                        ServerMessage::Accept { session_id, .. } => {
                            // Peer accepted our invite — set session to Active
                            if let Some(s) = chat_for_msg.write().sessions.get_mut(&session_id) {
                                s.status = SessionStatus::Active;
                            }
                            // Begin WebRTC handshake — offerer creates offer
                            if let Some(ref ws) = ws_for_webrtc.peek().ws {
                                let mut chat_dc = chat_for_msg;
                                let mut chat_rv = chat_for_msg;
                                match super::chat_client::wasm::setup_offerer(
                                    session_id.clone(),
                                    ws,
                                    move |sid, text| {
                                        if let Some(ctrl) = text.strip_prefix(super::chat_client::DC_CONTROL_PREFIX) {
                                            handle_dc_control(&mut chat_dc, &sid, ctrl);
                                            return;
                                        }
                                        let mut state = chat_dc.write();
                                        let peer_name = state.sessions.get(&sid)
                                            .map(|s| s.peer_name.clone())
                                            .unwrap_or_default();
                                        let msg = ChatMessage {
                                            sender_is_me: false,
                                            sender_name: peer_name,
                                            body: text,
                                            timestamp: chrono::Utc::now(),
                                        };
                                        if let Some(session) = state.sessions.get_mut(&sid) {
                                            session.messages.push(msg);
                                        }
                                    },
                                    move |sid| {
                                        web_sys::console::log_1(&format!("[WEBRTC] Offerer data channel ready for {}", sid).into());
                                    },
                                    move |sid, has_video| {
                                        if let Some(s) = chat_rv.write().sessions.get_mut(&sid) {
                                            s.has_remote_video = has_video;
                                        }
                                    },
                                ) {
                                    Ok(rtc_session) => {
                                        webrtc_for_msg.write().insert(session_id, rtc_session);
                                    }
                                    Err(e) => {
                                        web_sys::console::log_1(&format!("[WEBRTC] setup_offerer failed: {}", e).into());
                                    }
                                }
                            }
                        }
                        ServerMessage::Decline { session_id } => {
                            // Peer declined — remove the session we created
                            chat_for_msg.write().sessions.remove(&session_id);
                        }
                        ServerMessage::Text { session_id, ciphertext, .. } => {
                            // For now, treat ciphertext as plaintext (E2E encryption is a later phase)
                            let mut state = chat_for_msg.write();
                            let peer_name = state.sessions.get(&session_id)
                                .map(|s| s.peer_name.clone())
                                .unwrap_or_default();
                            let msg = ChatMessage {
                                sender_is_me: false,
                                sender_name: peer_name,
                                body: ciphertext,
                                timestamp: chrono::Utc::now(),
                            };
                            if let Some(session) = state.sessions.get_mut(&session_id) {
                                session.messages.push(msg);
                            }
                        }
                        ServerMessage::Close { session_id, reason } => {
                            chat_for_msg.write().sessions.remove(&session_id);
                            // Clean up WebRTC session
                            if let Some(rtc) = webrtc_for_close.write().remove(&session_id) {
                                super::chat_client::wasm::close_session(&rtc);
                            }
                            // Remove hidden audio element
                            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                                let audio_id = format!("remote-audio-{}", session_id);
                                if let Some(el) = doc.get_element_by_id(&audio_id) {
                                    el.remove();
                                }
                            }
                            web_sys::console::log_1(&format!("[CHAT] Session {} closed: {}", session_id, reason).into());
                        }
                        ServerMessage::Presence { pubkey, online } => {
                            chat_for_msg.write().peer_online.insert(pubkey, online);
                        }
                        ServerMessage::Sdp { session_id, sdp } => {
                            let sdp_type = sdp.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            if sdp_type == "answer" {
                                // We're the offerer — apply the answer
                                if let Some(rtc) = webrtc_for_msg.peek().get(&session_id) {
                                    super::chat_client::wasm::handle_sdp_answer(&rtc.pc, &sdp);
                                }
                            } else if sdp_type == "offer" {
                                // Check if we already have a WebRTC session for this ID
                                // (renegotiation after track add/remove)
                                let existing = webrtc_for_msg.peek().contains_key(&session_id);
                                if existing {
                                    if let Some(ref ws) = ws_for_webrtc.peek().ws {
                                        if let Some(rtc) = webrtc_for_msg.peek().get(&session_id) {
                                            super::chat_client::wasm::handle_renegotiation_offer(
                                                &rtc.pc, &sdp, ws, &session_id,
                                            );
                                        }
                                    }
                                } else {
                                    // Initial answerer setup — create new PeerConnection
                                    if let Some(ref ws) = ws_for_webrtc.peek().ws {
                                        let mut chat_dc = chat_for_msg;
                                        let mut chat_rv = chat_for_msg;
                                        match super::chat_client::wasm::setup_answerer(
                                            session_id.clone(),
                                            &sdp,
                                            ws,
                                            move |sid, text| {
                                                if let Some(ctrl) = text.strip_prefix(super::chat_client::DC_CONTROL_PREFIX) {
                                                    handle_dc_control(&mut chat_dc, &sid, ctrl);
                                                    return;
                                                }
                                                let mut state = chat_dc.write();
                                                let peer_name = state.sessions.get(&sid)
                                                    .map(|s| s.peer_name.clone())
                                                    .unwrap_or_default();
                                                let msg = ChatMessage {
                                                    sender_is_me: false,
                                                    sender_name: peer_name,
                                                    body: text,
                                                    timestamp: chrono::Utc::now(),
                                                };
                                                if let Some(session) = state.sessions.get_mut(&sid) {
                                                    session.messages.push(msg);
                                                }
                                            },
                                            move |sid| {
                                                web_sys::console::log_1(&format!("[WEBRTC] Answerer data channel ready for {}", sid).into());
                                            },
                                            move |sid, has_video| {
                                                if let Some(s) = chat_rv.write().sessions.get_mut(&sid) {
                                                    s.has_remote_video = has_video;
                                                }
                                            },
                                        ) {
                                            Ok(rtc_session) => {
                                                webrtc_for_msg.write().insert(session_id, rtc_session);
                                            }
                                            Err(e) => {
                                                web_sys::console::log_1(&format!("[WEBRTC] setup_answerer failed: {}", e).into());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ServerMessage::Ice { session_id, candidate } => {
                            if let Some(rtc) = webrtc_for_msg.peek().get(&session_id) {
                                super::chat_client::wasm::handle_ice_candidate(&rtc.pc, &candidate);
                            }
                        }
                        ServerMessage::Nonce { .. } => {}
                    }
                },
                move || {
                    chat_for_open.write().connected = true;
                },
                move || {
                    let mut state = chat_for_close.write();
                    state.connected = false;
                    state.authenticated = false;
                    state.peer_online.clear();
                    ws_for_close.write().ws = None;
                },
            ) {
                Ok(ws) => {
                    ws_handle.write().ws = Some(ws);
                }
                Err(e) => {
                    chat_state.write().last_error = Some(e);
                }
            }
        }
    });
}

#[component]
fn AppLayout() -> Element {
    let mut user_state = use_user_state();
    let mut key_manager: Signal<Option<KeyManager>> = use_context();
    let nav = use_navigator();

    let shared = use_shared_state();

    let state = user_state.read();
    let moniker = state.moniker.clone().unwrap_or_default();
    let order_count = state.orders.len();
    let is_customer = state.connected_supplier.is_some();
    let is_supplier = state.is_supplier;
    let is_root = state.is_root;
    let connected_supplier = state.connected_supplier.clone();
    drop(state);

    // Determine user role: Supplier (has products), User, or Guest
    let shared_read = shared.read();
    // Balance comes from the on-network user contract (root uses root_user_contract)
    let balance = if is_root {
        shared_read.root_user_contract.as_ref().map(|uc| uc.balance_curds).unwrap_or(0)
    } else {
        shared_read.user_contract.as_ref().map(|uc| uc.balance_curds).unwrap_or(0)
    };
    let has_products = shared_read
        .storefronts
        .get(&moniker)
        .map(|sf| !sf.products.is_empty())
        .unwrap_or(false);
    let role_label = if is_root {
        "Root"
    } else if has_products {
        "Supplier"
    } else if is_customer {
        "Guest"
    } else {
        "User"
    };

    // Compute displayed balance: base + incoming deposit credits for suppliers
    let incoming_deposits: u64 = if !is_customer && is_supplier {
        shared_read
            .storefronts
            .get(&moniker)
            .map(|sf| {
                sf.orders
                    .values()
                    .filter(|o| {
                        matches!(
                            o.status,
                            cream_common::order::OrderStatus::Reserved { .. }
                                | cream_common::order::OrderStatus::Paid
                        )
                    })
                    .map(|o| o.deposit_amount)
                    .sum()
            })
            .unwrap_or(0)
    } else {
        0
    };
    let is_connected = shared_read.connected;
    drop(shared_read);
    let displayed_balance = balance + incoming_deposits;

    // Connect to chat relay when KeyManager is available
    use_chat_connection();

    rsx! {
        div { class: "cream-app",
            header { class: "app-header",
                div { class: "header-top",
                    h1 { "CREAM " span { class: "tagline", "rises to the top" } }
                    div { class: "user-info",
                        Link {
                            class: "user-moniker clickable",
                            to: Route::Profile {},
                            "{moniker}"
                        }
                        span { class: "role-badge", " [{role_label}]" }
                        if is_connected {
                            span { class: "connection-badge connected", "Connected" }
                        } else {
                            span { class: "connection-badge disconnected", "Disconnected" }
                        }
                        button {
                            class: "iaq-btn",
                            onclick: move |_| { nav.push(Route::Faq {}); },
                            "FAQ"
                        }
                        button {
                            class: "iaq-btn",
                            onclick: move |_| { nav.push(Route::Iaq {}); },
                            "IAQ"
                        }
                        button {
                            class: "logout-btn",
                            onclick: move |_| {
                                UserState::clear_session();
                                key_manager.set(None);
                                user_state.set(UserState::new());
                            },
                            "Log out"
                        }
                    }
                }
                p { "The decentralized, private 24/7 farmer's market" }
                {
                    let inbox_count = shared.read().inbox.as_ref().map(|i| i.messages.len()).unwrap_or(0);
                    let admin_status = *use_context::<Signal<super::toll_rates::AdminStatus>>().read();
                    nav_buttons(nav.clone(), order_count, displayed_balance, is_supplier, connected_supplier.clone(), inbox_count, admin_status)
                }
            }
            ChatInviteBanner {}
            main {
                Outlet::<Route> {}
            }
            ChatPanel {}
        }
    }
}

/// Route component: renders the directory view.
#[component]
fn Directory() -> Element {
    let user_state = use_user_state();
    let connected = user_state.read().connected_supplier.clone();
    if let Some(supplier) = connected {
        let nav = use_navigator();
        nav.push(Route::Supplier { name: supplier });
        rsx! {}
    } else {
        rsx! { DirectoryView {} }
    }
}

/// Route component: renders a supplier's storefront by name from the URL.
#[component]
fn Supplier(name: String) -> Element {
    rsx! { StorefrontView { supplier_name: name } }
}

/// Route component: renders the orders view.
#[component]
fn Orders() -> Element {
    rsx! { MyOrders {} }
}

/// Route component: renders the messages/inbox view.
#[component]
fn Messages() -> Element {
    rsx! { MessagesView {} }
}

/// Route component: renders the supplier dashboard.
#[component]
fn Dashboard() -> Element {
    let user_state = use_user_state();
    let connected = user_state.read().connected_supplier.clone();
    if let Some(supplier) = connected {
        let nav = use_navigator();
        nav.push(Route::Supplier { name: supplier });
        rsx! {}
    } else {
        let is_supplier = user_state.read().is_supplier;

        if is_supplier {
            rsx! { SupplierDashboard {} }
        } else {
            rsx! { DirectoryView {} }
        }
    }
}

/// Route component: renders the markets listing (markets with upcoming events).
#[component]
fn Markets() -> Element {
    rsx! { MarketsListView {} }
}

/// Route component: renders a market detail view.
#[component]
fn Market(market_organizer: String) -> Element {
    rsx! { MarketView { market_organizer } }
}

/// Route component: renders the organizer's market dashboard.
#[component]
fn MyMarket() -> Element {
    rsx! { MarketDashboard {} }
}

/// Route component: renders the wallet view.
#[component]
fn Wallet() -> Element {
    rsx! { WalletView {} }
}

/// Route component: renders the FAQ.
#[component]
fn Faq() -> Element {
    rsx! { FaqView {} }
}

/// Route component: renders the IAQ documentation.
#[component]
fn Iaq() -> Element {
    rsx! { IaqView {} }
}

/// Route component: renders the guardian admin dashboard.
#[component]
fn Guardian() -> Element {
    rsx! { GuardianAdmin {} }
}

/// Route component: renders the user profile page.
#[component]
fn Profile() -> Element {
    rsx! { ProfileView {} }
}

/// Catch-all for unknown routes — redirects to directory (or storefront in customer mode).
#[component]
fn NotFound(segments: Vec<String>) -> Element {
    let nav = use_navigator();
    let user_state = use_user_state();
    if let Some(supplier) = user_state.read().connected_supplier.clone() {
        nav.push(Route::Supplier { name: supplier });
    } else {
        nav.push(Route::Directory {});
    }
    rsx! {}
}

// ─── Setup ───────────────────────────────────────────────────────────────────

#[component]
fn SetupScreen() -> Element {
    let mut key_manager: Signal<Option<KeyManager>> = use_context();
    let mut user_state = use_user_state();
    let _shared_state = use_shared_state();

    let mut name_input = use_signal(String::new);
    let mut postcode_input = use_signal(String::new);
    let mut is_supplier = use_signal(|| false);
    let mut supplier_desc = use_signal(String::new);
    let mut postcode_error = use_signal(|| None::<String>);

    // Locality selection
    let mut localities = use_signal(Vec::<PostcodeInfo>::new);
    let mut selected_locality = use_signal(|| None::<String>);

    // Returning user detection (supplier mode only)
    let mut welcome_back = use_signal(|| None::<String>);

    // Auto-connect via ?supplier= query param
    let url_supplier = use_signal(|| get_supplier_query_param());
    let auto_connect_mode = url_supplier.read().is_some();

    // Supplier name lookup (customer mode)
    let mut supplier_name_input = use_signal(|| {
        get_supplier_query_param().unwrap_or_default()
    });
    let mut supplier_lookup_error = use_signal(|| None::<String>);
    let mut supplier_lookup_loading = use_signal(|| false);
    let mut supplier_lookup_result =
        use_signal(|| None::<super::rendezvous::RendezvousEntry>);

    // Auto-trigger lookup when ?supplier= param is present (with retry for
    // transient network failures — wrangler dev server or Cloudflare cold starts).
    use_effect(move || {
        if let Some(name) = url_supplier.read().clone() {
            supplier_lookup_loading.set(true);
            spawn(async move {
                let mut last_err = String::new();
                for attempt in 0..3u32 {
                    if attempt > 0 {
                        gloo_timers::future::TimeoutFuture::new(1_000).await;
                    }
                    match super::rendezvous::lookup_supplier(&name).await {
                        Ok(entry) => {
                            supplier_lookup_result.set(Some(entry));
                            supplier_lookup_loading.set(false);
                            return;
                        }
                        Err(e) => {
                            last_err = e;
                        }
                    }
                }
                supplier_lookup_error.set(Some(last_err));
                supplier_lookup_loading.set(false);
            });
        }
    });

    let mut setup_error = use_signal(|| None::<String>);

    let node = use_node_action();

    let can_submit = {
                let name_ok = !name_input.read().trim().is_empty();
                let postcode_ok = is_valid_postcode(postcode_input.read().trim());
                let locality_ok = localities.read().len() <= 1
                    || selected_locality.read().is_some();
                let supplier_ok = if auto_connect_mode {
                    // In auto-connect mode, need a successful lookup
                    supplier_lookup_result.read().is_some()
                } else if *is_supplier.read() {
                    !supplier_desc.read().trim().is_empty()
                } else {
                    // Customer can proceed without selecting (browses directory),
                    // or with a resolved supplier (direct connect)
                    supplier_name_input.read().trim().is_empty()
                        || supplier_lookup_result.read().is_some()
                };
                name_ok && postcode_ok && locality_ok && supplier_ok
            };

            let localities_list = localities.read().clone();
            let current_locality = selected_locality.read().clone();
            let welcome_msg = welcome_back.read().clone();

            rsx! {
                div { class: "cream-app",
                    div { class: "user-setup",
                        h1 { "Welcome to CREAM" }
                        p { "The decentralized, private 24/7 farmer's market" }

                        div { class: "form-group",
                            label { "Your name:" }
                            input {
                                r#type: "text",
                                placeholder: "Name or moniker...",
                                value: "{name_input}",
                                oninput: move |evt| {
                                    let val = evt.value();
                                    name_input.set(val.clone());

                                    // Check directory for returning supplier
                                    let canonical = title_case(&val);
                                    let shared = _shared_state.read();
                                    let found = shared.directory.entries.values().find(|e| {
                                        e.name.eq_ignore_ascii_case(canonical.trim())
                                    });
                                    if let Some(entry) = found {
                                        is_supplier.set(true);
                                        supplier_desc.set(entry.description.clone());
                                        if let Some(pc) = entry.postcode.as_deref() {
                                            postcode_input.set(pc.to_string());
                                            let locs = lookup_all_localities(pc);
                                            if locs.len() == 1 {
                                                selected_locality.set(Some(locs[0].place_name.clone()));
                                            } else if let Some(loc) = entry.locality.as_deref() {
                                                // Auto-select matching locality
                                                if locs.iter().any(|l| l.place_name == loc) {
                                                    selected_locality.set(Some(loc.to_string()));
                                                } else {
                                                    selected_locality.set(None);
                                                }
                                            } else {
                                                selected_locality.set(None);
                                            }
                                            postcode_error.set(None);
                                            localities.set(locs);
                                        }
                                        welcome_back.set(Some(format!("Welcome back, {}!", entry.name)));
                                    } else {
                                        welcome_back.set(None);
                                    }
                                },
                            }
                        }

                        if let Some(msg) = welcome_msg.as_ref() {
                            p { class: "welcome-back", "{msg}" }
                        }

                        div { class: "form-group",
                            label { "Postcode (Australia):" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. 2000",
                                maxlength: "4",
                                value: "{postcode_input}",
                                oninput: move |evt| {
                                    let val = evt.value();
                                    postcode_input.set(val.clone());
                                    let trimmed = val.trim();
                                    if trimmed.is_empty() {
                                        postcode_error.set(None);
                                        localities.set(Vec::new());
                                        selected_locality.set(None);
                                    } else if is_valid_postcode(trimmed) {
                                        postcode_error.set(None);
                                        let locs = lookup_all_localities(trimmed);
                                        if locs.len() == 1 {
                                            selected_locality.set(Some(locs[0].place_name.clone()));
                                        } else {
                                            selected_locality.set(None);
                                        }
                                        localities.set(locs);
                                    } else {
                                        postcode_error.set(Some("Not a recognised postcode".into()));
                                        localities.set(Vec::new());
                                        selected_locality.set(None);
                                    }
                                },
                            }
                            if let Some(err) = postcode_error.read().as_ref() {
                                span { class: "field-error", "{err}" }
                            }
                        }

                        // Locality selection (only when multiple localities for the postcode)
                        if localities_list.len() > 1 {
                            div { class: "form-group",
                                label { "Locality:" }
                                select {
                                    value: current_locality.as_deref().unwrap_or(""),
                                    onchange: move |evt| {
                                        let val = evt.value();
                                        if val.is_empty() {
                                            selected_locality.set(None);
                                        } else {
                                            selected_locality.set(Some(val));
                                        }
                                    },
                                    option { value: "", "Select a locality..." }
                                    for loc in &localities_list {
                                        option {
                                            value: "{loc.place_name}",
                                            selected: current_locality.as_deref() == Some(loc.place_name.as_str()),
                                            "{loc.place_name}"
                                        }
                                    }
                                }
                            }
                        } else if localities_list.len() == 1 {
                            div { class: "form-group",
                                label { "Locality:" }
                                span { class: "locality-auto", "{localities_list[0].place_name}" }
                            }
                        }

                        if auto_connect_mode {
                            // Auto-connect via ?supplier= URL param
                            div { class: "form-group",
                                if *supplier_lookup_loading.read() {
                                    p { "Connecting to supplier: {supplier_name_input}..." }
                                } else if let Some(entry) = supplier_lookup_result.read().as_ref() {
                                    p { class: "welcome-back",
                                        "Connected to: {entry.name}"
                                    }
                                } else if let Some(err) = supplier_lookup_error.read().as_ref() {
                                    span { class: "field-error", "{err}" }
                                    // Fall back to manual lookup on error
                                    div { class: "supplier-lookup-row",
                                        input {
                                            r#type: "text",
                                            placeholder: "e.g. garys-farm",
                                            value: "{supplier_name_input}",
                                            oninput: move |evt| {
                                                supplier_name_input.set(evt.value());
                                                supplier_lookup_error.set(None);
                                                supplier_lookup_result.set(None);
                                            },
                                        }
                                        button {
                                            disabled: supplier_name_input.read().trim().is_empty()
                                                || *supplier_lookup_loading.read(),
                                            onclick: move |_| {
                                                let name = supplier_name_input.read().trim().to_string();
                                                if name.is_empty() {
                                                    return;
                                                }
                                                supplier_lookup_loading.set(true);
                                                supplier_lookup_error.set(None);
                                                supplier_lookup_result.set(None);
                                                spawn(async move {
                                                    match super::rendezvous::lookup_supplier(&name).await {
                                                        Ok(entry) => {
                                                            supplier_lookup_result.set(Some(entry));
                                                        }
                                                        Err(e) => {
                                                            supplier_lookup_error.set(Some(e));
                                                        }
                                                    }
                                                    supplier_lookup_loading.set(false);
                                                });
                                            },
                                            if *supplier_lookup_loading.read() {
                                                "Looking up..."
                                            } else {
                                                "Look up"
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            // Normal flow: supplier checkbox + manual lookup
                            div { class: "form-group",
                                label {
                                    input {
                                        r#type: "checkbox",
                                        checked: *is_supplier.read(),
                                        onchange: move |evt| {
                                            is_supplier.set(evt.checked());
                                            if evt.checked() {
                                                // Clear supplier lookup state when switching to supplier mode
                                                supplier_name_input.set(String::new());
                                                supplier_lookup_error.set(None);
                                                supplier_lookup_result.set(None);
                                            }
                                        },
                                    }
                                    " I want to sell products (register as supplier)"
                                }
                            }

                            if *is_supplier.read() {
                                div { class: "form-group",
                                    label { "Storefront description:" }
                                    textarea {
                                        placeholder: "Describe your farm or dairy...",
                                        value: "{supplier_desc}",
                                        oninput: move |evt| supplier_desc.set(evt.value()),
                                    }
                                }
                            }

                            if !*is_supplier.read() {
                                // Nearby suppliers from directory
                                {
                                    let nearby: Vec<(String, f64)> = lookup_postcode(postcode_input.read().trim())
                                        .map(|loc| {
                                            let dir_entries = &_shared_state.read().directory.entries;
                                            let mut list: Vec<_> = dir_entries.values()
                                                .map(|e| (e.name.clone(), e.location.distance_km(&loc)))
                                                .collect();
                                            list.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                                            list.truncate(5);
                                            list
                                        })
                                        .unwrap_or_default();
                                    rsx! {
                                        div { class: "form-group",
                                            label { "Connect to a supplier:" }
                                            if !nearby.is_empty() {
                                                select {
                                                    value: "{supplier_name_input}",
                                                    onchange: move |evt: Event<FormData>| {
                                                        let name = evt.value();
                                                        if name.is_empty() {
                                                            supplier_name_input.set(String::new());
                                                            supplier_lookup_error.set(None);
                                                            supplier_lookup_result.set(None);
                                                            return;
                                                        }
                                                        supplier_name_input.set(name.clone());
                                                        supplier_lookup_loading.set(true);
                                                        supplier_lookup_error.set(None);
                                                        supplier_lookup_result.set(None);
                                                        spawn(async move {
                                                            match super::rendezvous::lookup_supplier(&name).await {
                                                                Ok(entry) => {
                                                                    supplier_lookup_result.set(Some(entry));
                                                                }
                                                                Err(e) => {
                                                                    supplier_lookup_error.set(Some(e));
                                                                }
                                                            }
                                                            supplier_lookup_loading.set(false);
                                                        });
                                                    },
                                                    option { value: "", "Select a supplier..." }
                                                    for (name, dist) in nearby {
                                                        option {
                                                            value: "{name}",
                                                            selected: *supplier_name_input.read() == name,
                                                            "{name} — {dist:.0}km"
                                                        }
                                                    }
                                                }
                                            } else if is_valid_postcode(postcode_input.read().trim()) {
                                                p { class: "lookup-status", "No suppliers found nearby" }
                                            }
                                            if *supplier_lookup_loading.read() {
                                                p { class: "lookup-status", "Connecting to supplier..." }
                                            }
                                            if let Some(err) = supplier_lookup_error.read().as_ref() {
                                                span { class: "field-error", "{err}" }
                                            }
                                            if let Some(entry) = supplier_lookup_result.read().as_ref() {
                                                p { class: "welcome-back",
                                                    "Connected to: {entry.name}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(err) = setup_error.read().as_ref() {
                            p { class: "field-error", "{err}" }
                        }

                        button {
                            disabled: !can_submit,
                            onclick: move |_| {
                                let name = title_case(&name_input.read());
                                let pw = name.to_lowercase();

                                // Detect root login: "root" uses the system root key
                                let logging_in_as_root = name.eq_ignore_ascii_case("root");

                                let km = if logging_in_as_root {
                                    KeyManager::for_root()
                                } else {
                                    match KeyManager::from_credentials(&name, &pw) {
                                        Ok(km) => km,
                                        Err(e) => {
                                            setup_error.set(Some(format!("{e}")));
                                            return;
                                        }
                                    }
                                };

                                let postcode = postcode_input.read().trim().to_string();
                                let locality_val = selected_locality.read().clone();
                                let is_sup = *is_supplier.read();
                                let desc = supplier_desc.read().trim().to_string();
                                let lookup_result = supplier_lookup_result.read().clone();

                                {
                                    let mut state = user_state.write();
                                    state.moniker = Some(name.clone());
                                    state.postcode = Some(postcode.clone());
                                    state.locality = locality_val.clone();
                                    state.is_root = logging_in_as_root;
                                    if let Some(entry) = lookup_result.as_ref() {
                                        state.is_supplier = false;
                                        state.connected_supplier = Some(entry.name.clone());
                                        state.supplier_node_url = Some(entry.address.clone());
                                        state.supplier_storefront_key = Some(entry.storefront_key.clone());
                                    } else {
                                        state.is_supplier = is_sup;
                                        if is_sup {
                                            state.supplier_description = if desc.is_empty() {
                                                None
                                            } else {
                                                Some(desc.clone())
                                            };
                                        }
                                    }
                                    state.save();
                                }

                                UserState::save_password(&pw);
                                key_manager.set(Some(km));

                                // Root user doesn't need a separate user contract — they
                                // use the system root contract which is already subscribed.
                                if logging_in_as_root {
                                    // no RegisterUser, no RegisterSupplier
                                } else if lookup_result.is_none() && is_sup {
                                    node.send(NodeAction::RegisterSupplier {
                                        name: name.clone(),
                                        postcode,
                                        locality: locality_val,
                                        description: desc,
                                    });
                                    // Deploy user contract for the supplier (every supplier is also a user)
                                    node.send(NodeAction::RegisterUser {
                                        name: name.clone(),
                                        origin_supplier: name.clone(),
                                        current_supplier: name.clone(),
                                        invited_by: cream_common::identity::ROOT_USER_NAME.to_string(),
                                    });
                                } else if let Some(entry) = lookup_result.as_ref() {
                                    node.send(NodeAction::SubscribeCustomerStorefront {
                                        storefront_key: entry.storefront_key.clone(),
                                    });
                                    // Deploy user contract for the customer
                                    node.send(NodeAction::RegisterUser {
                                        name: name.clone(),
                                        origin_supplier: entry.name.clone(),
                                        current_supplier: entry.name.clone(),
                                        invited_by: entry.name.clone(),
                                    });
                                } else {
                                    // Standalone customer (browses directory, not connected to a supplier)
                                    node.send(NodeAction::RegisterUser {
                                        name: name.clone(),
                                        origin_supplier: String::new(),
                                        current_supplier: String::new(),
                                        invited_by: cream_common::identity::ROOT_USER_NAME.to_string(),
                                    });
                                }
                            },
                            "Get Started"
                        }
                    }
                }
            }
}
