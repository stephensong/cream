use dioxus::prelude::*;

/// Actions the UI can send to the Freenet node via the coroutine.
#[derive(Debug, Clone)]
#[allow(dead_code)] // variants used in WASM builds only
pub enum NodeAction {
    /// Register as a supplier in the directory contract.
    RegisterSupplier {
        name: String,
        postcode: String,
        locality: Option<String>,
        description: String,
    },
    /// Deploy a new storefront contract for this supplier.
    #[allow(dead_code)] // handled via RegisterSupplier for now
    DeployStorefront {
        name: String,
        description: String,
        latitude: f64,
        longitude: f64,
    },
    /// Add a product to the supplier's storefront.
    AddProduct {
        name: String,
        category: String,
        description: String,
        price_curd: u64,
        quantity_total: u32,
    },
    /// Remove a product from the storefront.
    #[allow(dead_code)] // TODO: implement
    RemoveProduct { product_id: String },
    /// Place an order on a storefront.
    PlaceOrder {
        storefront_name: String,
        product_id: String,
        quantity: u32,
        deposit_tier: String,
        price_per_unit: u64,
        collection_point: Option<cream_common::order::CollectionPoint>,
    },
    /// Subscribe to a specific storefront's updates.
    #[allow(dead_code)] // auto-subscribed via directory; kept for manual use
    SubscribeStorefront { supplier_name: String },
    /// Customer mode: subscribe to the supplier's storefront after setup completes.
    SubscribeCustomerStorefront { storefront_key: String },
    /// Update the supplier's opening hours schedule.
    UpdateSchedule {
        schedule: cream_common::storefront::WeeklySchedule,
    },
    /// Cancel an order on the supplier's storefront (refund deposit).
    CancelOrder { order_id: String },
    /// Fulfill an order: transition to Fulfilled and settle escrowed deposit to supplier.
    FulfillOrder { order_id: String },
    /// Update a product's price and/or quantity on the supplier's storefront.
    UpdateProduct {
        product_id: String,
        price_curd: u64,
        quantity_total: u32,
    },
    /// Update supplier contact details (phone, email, address).
    UpdateContactDetails {
        phone: Option<String>,
        email: Option<String>,
        address: Option<String>,
    },
    /// Deploy a user contract for the current user.
    RegisterUser {
        name: String,
        origin_supplier: String,
        current_supplier: String,
        invited_by: String,
    },
    /// Update the user's contract state (supplier change).
    UpdateUserContract {
        current_supplier: Option<String>,
    },
    /// Peg-in: deposit BTC via Lightning, receive CURD (mock/dev mode).
    PegIn { amount_sats: u64 },
    /// Peg-in step: CURD allocation after Lightning invoice is accepted.
    /// Called by the wallet_view polling loop once the hold invoice is paid.
    PegInAllocate {
        amount_sats: u64,
        payment_hash: String,
    },
    /// Peg-out: burn CURD and withdraw BTC via Lightning (mock/dev mode).
    PegOut { amount_curd: u64, bolt11: String },
    /// Peg-out via real Lightning: debit CURD first, then pay via gateway.
    PegOutViaGateway { amount_curd: u64, bolt11: String },
    /// Faucet: transfer 1000 CURD from root to the current user.
    FaucetTopUp,
    /// Send a message to a user's inbox contract (costs 10 CURD toll).
    SendInboxMessage {
        recipient_name: String,
        body: String,
        kind: cream_common::inbox::MessageKind,
        /// For non-directory recipients (root, admins): their ed25519 pubkey hex.
        /// When set, the inbox contract key is computed from this instead of directory lookup.
        recipient_pubkey_hex: Option<String>,
    },
    /// Session toll: initiator pays per interval to root/guardians.
    SessionToll,
    /// Peer-to-peer transfer: user → peer's contract (for request-to-pay).
    PeerTransfer {
        peer_pubkey_hex: String,
        amount: u64,
        description: String,
    },
    /// Update toll rates on the root user contract (admin only, FROST-signed).
    SetTollRates {
        rates: cream_common::tolls::TollRates,
    },
    /// Register a new market in the market directory.
    RegisterMarket {
        name: String,
        description: String,
        venue_address: String,
        postcode: String,
        locality: Option<String>,
        timezone: Option<String>,
    },
    /// Invite a supplier to participate in the organizer's market.
    InviteMarketSupplier {
        supplier_name: String,
    },
    /// Supplier accepts a market invitation (sends MarketAccept inbox to organizer).
    AcceptMarketInvite {
        market_name: String,
    },
    /// Auto-confirm: flip Invited → Accepted for a supplier (organizer-side).
    ConfirmMarketAcceptance {
        supplier_name: String,
    },
    /// Update market event dates.
    UpdateMarketEvents {
        events: Vec<cream_common::market::MarketEvent>,
    },
    /// Update market details (name, description, venue, location, timezone).
    UpdateMarketDetails {
        name: String,
        description: String,
        venue_address: String,
        postcode: String,
        locality: Option<String>,
        timezone: Option<String>,
    },
    /// Supplier sets which of their products are available at a specific market.
    UpdateMarketProducts {
        market_name: String,
        product_ids: std::collections::BTreeSet<String>,
    },
    /// Checkpoint the user's ledger: fold old transactions into checkpoint_balance.
    CheckpointLedger,
}

/// Get a handle to send actions to the node communication coroutine.
#[allow(dead_code)] // used in WASM builds only
pub fn use_node_action() -> Coroutine<NodeAction> {
    use_coroutine_handle::<NodeAction>()
}

/// Start the node communication coroutine.
///
/// Connects via WebSocket to a Freenet node and handles contract operations.
pub fn use_node_coroutine() {
    use_coroutine(|rx: UnboundedReceiver<NodeAction>| node_comms(rx));
}

// ─── WASM re-exports for wallet backend ─────────────────────────────────────

#[cfg(target_family = "wasm")]
pub(crate) use wasm_impl::{generate_tx_ref, now_iso8601, record_transfer, ContractRole};

// ─── WASM implementation ────────────────────────────────────────────────────

#[cfg(target_family = "wasm")]
mod wasm_impl {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Arc;

    use dioxus::prelude::*;
    use futures::channel::mpsc;
    use futures::{SinkExt, StreamExt};
    use wasm_bindgen::JsCast;

    use cream_common::directory::{DirectoryEntry, DirectoryState};
    use cream_common::location::GeoLocation;
    use cream_common::order::{DepositTier, Order, OrderId, OrderStatus};
    use cream_common::product::{Product, ProductCategory, ProductId};
    use cream_common::storefront::{
        SignedProduct, StorefrontInfo, StorefrontParameters, StorefrontState,
    };
    use cream_common::user_contract::{UserContractParameters, UserContractState};
    use freenet_stdlib::client_api::{
        ClientError, ClientRequest, ContractRequest, ContractResponse, HostResponse,
    };
    use freenet_stdlib::prelude::*;

    use super::NodeAction;
    use crate::components::key_manager::KeyManager;
    use crate::components::shared_state::use_shared_state;
    use crate::components::wallet_native::CreamNativeWallet;

    /// Sleep for the given number of milliseconds (WASM-compatible).
    #[allow(dead_code)] // used in supplier mode heartbeat
    async fn gloo_timers_sleep(ms: u32) {
        let (tx, rx) = futures::channel::oneshot::channel::<()>();
        let closure = wasm_bindgen::closure::Closure::once(move || {
            let _ = tx.send(());
        });
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                ms as i32,
            )
            .unwrap();
        closure.forget();
        let _ = rx.await;
    }

    /// Log a message to the browser console.
    fn clog(msg: &str) {
        web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(msg));
    }

    /// Generate a random u32 using JS Math.random().
    fn rand_u32() -> u32 {
        (web_sys::js_sys::Math::random() * u32::MAX as f64) as u32
    }

    /// Embedded directory contract WASM (built with `cargo make build-contracts-dev`).
    const DIRECTORY_CONTRACT_WASM: &[u8] = include_bytes!(
        "../../../target/wasm32-unknown-unknown/release/cream_directory_contract.wasm"
    );

    /// Embedded storefront contract WASM (built with `cargo make build-contracts-dev`).
    const STOREFRONT_CONTRACT_WASM: &[u8] = include_bytes!(
        "../../../target/wasm32-unknown-unknown/release/cream_storefront_contract.wasm"
    );

    /// Embedded user contract WASM (built with `cargo make build-contracts-dev`).
    const USER_CONTRACT_WASM: &[u8] = include_bytes!(
        "../../../target/wasm32-unknown-unknown/release/cream_user_contract.wasm"
    );

    /// Embedded inbox contract WASM (built with `cargo make build-contracts-dev`).
    const INBOX_CONTRACT_WASM: &[u8] = include_bytes!(
        "../../../target/wasm32-unknown-unknown/release/cream_inbox_contract.wasm"
    );

    /// Embedded market directory contract WASM (built with `cargo make build-contracts-dev`).
    const MARKET_DIRECTORY_CONTRACT_WASM: &[u8] = include_bytes!(
        "../../../target/wasm32-unknown-unknown/release/cream_market_directory_contract.wasm"
    );

    /// Build a ContractContainer from raw WASM bytes and parameters.
    fn make_contract(
        wasm_bytes: &[u8],
        params: Parameters<'static>,
    ) -> ContractContainer {
        let code = ContractCode::from(wasm_bytes.to_vec());
        let wrapped = WrappedContract::new(Arc::new(code), params);
        ContractContainer::Wasm(ContractWasmAPIVersion::V1(wrapped))
    }

    /// Main node communication loop.
    pub async fn node_comms(mut rx: UnboundedReceiver<NodeAction>) {
        let mut shared = use_shared_state();
        let key_manager_signal: Signal<Option<KeyManager>> = use_context();
        let user_state: Signal<crate::components::user_state::UserState> = use_context();
        let toll_rates: Signal<cream_common::tolls::TollRates> = use_context();

        // ── Connect to node via WebSocket ───────────────────────────────
        // Default node URL; overridable at compile-time via CREAM_NODE_URL env var,
        // or at runtime via ?node=<port> query parameter (e.g. ?node=3003).
        // In customer mode, the supplier's node URL from UserState takes priority.
        const DEFAULT_NODE_URL: &str =
            "ws://localhost:3001/v1/contract/command?encodingProtocol=native";
        let compile_time_url = option_env!("CREAM_NODE_URL").unwrap_or(DEFAULT_NODE_URL);

        let node_url = {
            // Customer mode: use supplier node URL from UserState if available
            let customer_url = user_state.read().supplier_node_url.clone();

            if let Some(url) = customer_url {
                url
            } else {
                let url = web_sys::window()
                    .and_then(|w| w.location().search().ok())
                    .and_then(|qs| {
                        web_sys::UrlSearchParams::new_with_str(&qs)
                            .ok()?
                            .get("node")
                    })
                    .map(|port| {
                        format!(
                            "ws://localhost:{port}/v1/contract/command?encodingProtocol=native"
                        )
                    });
                match url {
                    Some(u) => u,
                    None => compile_time_url.to_string(),
                }
            }
        };

        let conn = match web_sys::WebSocket::new(&node_url) {
            Ok(c) => c,
            Err(e) => {
                shared.write().last_error =
                    Some(format!("WebSocket connection failed: {:?}", e));
                return;
            }
        };

        let (send_responses, mut host_responses) = mpsc::unbounded();
        let (send_half, mut requests) = mpsc::unbounded::<ClientRequest<'static>>();

        let result_handler = {
            let sender = send_responses.clone();
            move |result: Result<HostResponse, ClientError>| {
                let mut sender = sender.clone();
                let _ = wasm_bindgen_futures::future_to_promise(async move {
                    sender.send(result).await.expect("channel open");
                    Ok(wasm_bindgen::JsValue::NULL)
                });
            }
        };

        let (tx_connected, rx_connected) = futures::channel::oneshot::channel();
        let onopen_handler = move || {
            let _ = tx_connected.send(());
            tracing::info!("Connected to Freenet node");
        };

        let mut api = freenet_stdlib::client_api::WebApi::start(
            conn,
            result_handler,
            |err| {
                tracing::error!("Node error: {err}");
            },
            onopen_handler,
        );

        // Wait for connection
        if rx_connected.await.is_err() {
            shared.write().last_error = Some("WebSocket connection dropped".into());
            return;
        }
        shared.write().connected = true;
        clog("[CREAM] Connected to Freenet node");

        // ── Set up contracts ─────────────────────────────────────────
        let directory_contract =
            make_contract(DIRECTORY_CONTRACT_WASM, Parameters::from(vec![]));
        let directory_key = directory_contract.key();

        let is_customer = user_state.read().connected_supplier.is_some();

        // In customer mode, we use a dummy directory instance ID that will never
        // match any response, since we skip directory operations entirely.
        let directory_instance_id = if is_customer {
            // Customer mode: storefront subscription is triggered by
            // SubscribeCustomerStorefront action after setup completes.
            // Use a zeroed-out dummy so no response matches the directory branch.
            ContractInstanceId::new([0u8; 32])
        } else {
            let id = *directory_key.id();

            tracing::info!("Directory contract key: {:?}", directory_key);
            shared.write().directory_contract_key =
                Some(format!("{}", directory_key));

            // Try to GET the existing directory.
            // If it doesn't exist yet, we'll PUT it when we get NotFound.
            let get_request = ClientRequest::ContractOp(ContractRequest::Get {
                key: id,
                return_contract_code: false,
                subscribe: false,
                blocking_subscribe: false,
            });

            tracing::info!("Getting directory contract (will PUT if not found)...");
            if let Err(e) = api.send(get_request).await {
                tracing::error!("Failed to GET directory contract: {:?}", e);
                shared.write().last_error =
                    Some(format!("Failed to get directory contract: {:?}", e));
            }

            // Explicitly subscribe to directory updates
            let subscribe_dir = ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: id,
                summary: None,
            });
            tracing::info!("Subscribing to directory contract...");
            if let Err(e) = api.send(subscribe_dir).await {
                tracing::error!("Failed to subscribe to directory: {:?}", e);
            }

            id
        };

        // ── Set up market directory contract ───────────────────────────
        let market_directory_contract =
            make_contract(MARKET_DIRECTORY_CONTRACT_WASM, Parameters::from(vec![]));
        let market_directory_key = market_directory_contract.key();

        let market_directory_instance_id = if is_customer {
            ContractInstanceId::new([0u8; 32])
        } else {
            let id = *market_directory_key.id();

            tracing::info!("Market directory contract key: {:?}", market_directory_key);
            shared.write().market_directory_key =
                Some(format!("{}", market_directory_key));

            let get_request = ClientRequest::ContractOp(ContractRequest::Get {
                key: id,
                return_contract_code: false,
                subscribe: false,
                blocking_subscribe: false,
            });
            if let Err(e) = api.send(get_request).await {
                tracing::error!("Failed to GET market directory contract: {:?}", e);
            }

            let subscribe_mkt = ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: id,
                summary: None,
            });
            if let Err(e) = api.send(subscribe_mkt).await {
                tracing::error!("Failed to subscribe to market directory: {:?}", e);
            }

            id
        };

        // Local map of supplier name -> ContractKey for storefront updates
        let mut sf_contract_keys: BTreeMap<String, ContractKey> = BTreeMap::new();

        // Track which storefronts we've already subscribed to
        let mut subscribed_storefronts: HashSet<ContractInstanceId> = HashSet::new();

        // Forward lookup: ContractInstanceId → directory entry name.
        // Populated when we subscribe to storefronts so we can correctly
        // key incoming GET/Update responses by the directory name (e.g. "Gary")
        // rather than the storefront's own info.name (e.g. "Gary's Farm").
        let mut instance_to_name: std::collections::HashMap<ContractInstanceId, String> = std::collections::HashMap::new();

        // Track the user's own user contract instance ID for response routing.
        let mut user_contract_instance_id: Option<ContractInstanceId> = None;
        // Track the user contract key for updates.
        let mut user_contract_key: Option<ContractKey> = None;
        // Track the user's inbox contract instance ID for response routing.
        let mut inbox_contract_instance_id: Option<ContractInstanceId> = None;
        // Track the inbox contract key for updates.
        let mut inbox_contract_key: Option<ContractKey> = None;

        // On startup, if we have a saved user contract key, GET + subscribe to it.
        {
            let saved_key = user_state.read().user_contract_key.clone();
            if let Some(key_str) = saved_key {
                if let Ok(instance_id) = ContractInstanceId::from_bytes(&key_str) {
                    clog(&format!("[CREAM] Restoring user contract subscription: {}", key_str));
                    user_contract_instance_id = Some(instance_id);
                    let get_req = ClientRequest::ContractOp(ContractRequest::Get {
                        key: instance_id,
                        return_contract_code: false,
                        subscribe: false,
                        blocking_subscribe: false,
                    });
                    if let Err(e) = api.send(get_req).await {
                        clog(&format!("[CREAM] ERROR: Failed to GET user contract: {:?}", e));
                    }
                    let sub_req = ClientRequest::ContractOp(ContractRequest::Subscribe {
                        key: instance_id,
                        summary: None,
                    });
                    if let Err(e) = api.send(sub_req).await {
                        clog(&format!("[CREAM] ERROR: Failed to subscribe to user contract: {:?}", e));
                    }
                }
            }
        }

        // On startup, deploy (PUT) our inbox contract if it doesn't exist,
        // then subscribe to it. PUT is idempotent — if the contract already
        // exists, Freenet merges the empty state harmlessly.
        {
            let km = key_manager_signal.read().clone();
            if let Some(ref km) = km {
                let owner_key = km.verifying_key();
                let inbox_params = cream_common::inbox::InboxParameters { owner: owner_key };
                let params_bytes = serde_json::to_vec(&inbox_params).unwrap();
                let inbox_container = make_contract(INBOX_CONTRACT_WASM, Parameters::from(params_bytes));
                let ib_key = inbox_container.key();
                let ib_instance_id = *ib_key.id();

                clog(&format!("[CREAM] Inbox contract key: {}", ib_key));
                inbox_contract_instance_id = Some(ib_instance_id);
                inbox_contract_key = Some(ib_key);

                let ib_state = cream_common::inbox::InboxState {
                    owner: km.user_id(),
                    messages: std::collections::BTreeMap::new(),
                    updated_at: chrono::Utc::now(),
                    extra: Default::default(),
                };
                let ib_state_bytes = serde_json::to_vec(&ib_state).unwrap();

                // PUT with subscribe=true ensures the contract exists AND we're subscribed
                let put_inbox = ClientRequest::ContractOp(ContractRequest::Put {
                    contract: inbox_container,
                    state: WrappedState::new(ib_state_bytes.clone()),
                    related_contracts: RelatedContracts::default(),
                    subscribe: true,
                    blocking_subscribe: false,
                });
                if let Err(e) = api.send(put_inbox).await {
                    clog(&format!("[CREAM] WARNING: Failed to PUT inbox contract: {:?}", e));
                }

                {
                    let mut state = shared.write();
                    state.inbox = Some(ib_state);
                    state.inbox_contract_key = Some(format!("{}", ib_key));
                }
            }
        }

        // ── Subscribe to root user contract ─────────────────────────────
        // Root's identity is deterministic, so we can derive its contract key.
        let root_vk = cream_common::identity::root_user_id().0;
        let root_params = UserContractParameters { owner: root_vk };
        let root_params_bytes = serde_json::to_vec(&root_params).unwrap();
        let root_contract_container = make_contract(USER_CONTRACT_WASM, Parameters::from(root_params_bytes));
        let root_contract_full_key: ContractKey = root_contract_container.key();
        let root_contract_instance_id: Option<ContractInstanceId> = {
            let root_key_str = format!("{}", root_contract_full_key);
            let root_instance = *root_contract_full_key.id();

            clog(&format!("[CREAM] Root contract key: {}", root_key_str));
            shared.write().root_contract_key = Some(root_key_str);

            // GET + subscribe to root contract
            let get_root = ClientRequest::ContractOp(ContractRequest::Get {
                key: root_instance,
                return_contract_code: false,
                subscribe: false,
                blocking_subscribe: false,
            });
            if let Err(e) = api.send(get_root).await {
                clog(&format!("[CREAM] ERROR: Failed to GET root contract: {:?}", e));
            }
            let sub_root = ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: root_instance,
                summary: None,
            });
            if let Err(e) = api.send(sub_root).await {
                clog(&format!("[CREAM] ERROR: Failed to subscribe to root contract: {:?}", e));
            }

            Some(root_instance)
        };

        // ── Create signing service ───────────────────────────────────────
        let signing_service = crate::components::signing_service::SigningService::from_env();

        // ── Main event loop ─────────────────────────────────────────────
        loop {
            futures::select! {
                action = rx.next() => {
                    let Some(action) = action else { break };
                    let km = key_manager_signal.read().clone();
                    let Some(km) = km else {
                        tracing::warn!("Action received but no KeyManager available, dropping: {:?}", action);
                        continue;
                    };
                    handle_action(
                        action,
                        &mut api,
                        &mut shared,
                        &directory_key,
                        &mut sf_contract_keys,
                        &km,
                        &node_url,
                        is_customer,
                        &user_state,
                        &send_half,
                        &mut user_contract_instance_id,
                        &mut user_contract_key,
                        &root_contract_full_key,
                        &signing_service,
                        &mut inbox_contract_instance_id,
                        &mut inbox_contract_key,
                        &toll_rates,
                        &market_directory_key,
                    ).await;
                }

                response = host_responses.next() => {
                    let Some(response) = response else { break };
                    match response {
                        Ok(HostResponse::ContractResponse(cr)) => {
                            let csn = user_state.read().connected_supplier.clone();
                            let follow_ups = handle_contract_response(
                                &mut shared, cr, directory_instance_id,
                                &mut subscribed_storefronts,
                                &mut instance_to_name,
                                &mut sf_contract_keys,
                                csn.as_deref(),
                                user_contract_instance_id,
                                root_contract_instance_id,
                                inbox_contract_instance_id,
                                market_directory_instance_id,
                            );
                            for follow_up in follow_ups {
                                if let Err(e) = api.send(follow_up).await {
                                    tracing::error!("Failed to send follow-up: {:?}", e);
                                }
                            }
                        }
                        Ok(HostResponse::Ok) => {
                            tracing::debug!("Node OK");
                        }
                        Ok(other) => {
                            clog(&format!("[CREAM] Unhandled response: {:?}", other));
                        }
                        Err(e) => {
                            // Check if this is a MissingContract error for the
                            // directory — treat it like NotFound and PUT.
                            let is_missing_directory = matches!(
                                e.kind(),
                                freenet_stdlib::client_api::ErrorKind::RequestError(
                                    freenet_stdlib::client_api::RequestError::ContractError(
                                        freenet_stdlib::client_api::ContractError::MissingContract { key }
                                    )
                                ) if *key == directory_instance_id
                            );
                            let is_missing_market_directory = matches!(
                                e.kind(),
                                freenet_stdlib::client_api::ErrorKind::RequestError(
                                    freenet_stdlib::client_api::RequestError::ContractError(
                                        freenet_stdlib::client_api::ContractError::MissingContract { key }
                                    )
                                ) if *key == market_directory_instance_id
                            );
                            if is_missing_directory {
                                tracing::info!("Directory contract missing, creating it...");
                                let dir_contract = make_contract(
                                    DIRECTORY_CONTRACT_WASM,
                                    Parameters::from(vec![]),
                                );
                                let empty_dir = DirectoryState::default();
                                let initial_state =
                                    serde_json::to_vec(&empty_dir).unwrap();
                                let put_req = ClientRequest::ContractOp(
                                    ContractRequest::Put {
                                        contract: dir_contract,
                                        state: WrappedState::new(initial_state),
                                        related_contracts: RelatedContracts::default(),
                                        subscribe: true,
                                        blocking_subscribe: false,
                                    },
                                );
                                if let Err(e) = api.send(put_req).await {
                                    tracing::error!("Failed to PUT directory: {:?}", e);
                                }
                            } else if is_missing_market_directory {
                                tracing::info!("Market directory contract missing, creating it...");
                                let mkt_contract = make_contract(
                                    MARKET_DIRECTORY_CONTRACT_WASM,
                                    Parameters::from(vec![]),
                                );
                                let empty_mkt = cream_common::market::MarketDirectoryState::default();
                                let initial_state =
                                    serde_json::to_vec(&empty_mkt).unwrap();
                                let put_req = ClientRequest::ContractOp(
                                    ContractRequest::Put {
                                        contract: mkt_contract,
                                        state: WrappedState::new(initial_state),
                                        related_contracts: RelatedContracts::default(),
                                        subscribe: true,
                                        blocking_subscribe: false,
                                    },
                                );
                                if let Err(e) = api.send(put_req).await {
                                    tracing::error!("Failed to PUT market directory: {:?}", e);
                                }
                            } else {
                                clog(&format!("[CREAM] Node error: {:?}", e));
                                shared.write().last_error = Some(format!("{:?}", e));
                            }
                        }
                    }
                }

                request = requests.next() => {
                    let Some(request) = request else { break };
                    if let Err(e) = api.send(request).await {
                        tracing::error!("Failed to send request: {:?}", e);
                        shared.write().last_error =
                            Some(format!("Send failed: {:?}", e));
                    }
                }
            }
        }

        shared.write().connected = false;
    }

    /// Generate a unique transaction reference string.
    pub(crate) fn generate_tx_ref(sender: &str) -> String {
        let ts = web_sys::js_sys::Date::now() as u64;
        let rnd = rand_u32();
        format!("{}:{}:{}", sender, ts, rnd)
    }

    /// Get current time as ISO 8601 string.
    pub(crate) fn now_iso8601() -> String {
        web_sys::js_sys::Date::new_0().to_iso_string().into()
    }

    /// Identifies a user contract by role (for record_transfer).
    pub(crate) enum ContractRole {
        Root,
        User,
        ThirdParty(ContractKey),
    }

    /// Record a double-entry transfer between two user contracts.
    ///
    /// Appends a debit to the sender's contract and a credit to the receiver's contract,
    /// linked by a shared `tx_ref`. Both contracts are updated on the network.
    pub(crate) async fn record_transfer(
        api: &mut freenet_stdlib::client_api::WebApi,
        shared: &mut Signal<crate::components::shared_state::SharedState>,
        sender: ContractRole,
        receiver: ContractRole,
        root_contract_key: &ContractKey,
        user_contract_key: Option<&ContractKey>,
        amount: u64,
        description: String,
        sender_name: String,
        receiver_name: String,
        override_tx_ref: Option<String>,
        signing_service: &crate::components::signing_service::SigningService,
        lightning_payment_hash: Option<String>,
    ) {
        let tx_ref = override_tx_ref.unwrap_or_else(|| generate_tx_ref(&sender_name));
        let timestamp = now_iso8601();

        // Build debit entry (for sender's contract)
        let debit = cream_common::wallet::WalletTransaction {
            id: 0,
            kind: cream_common::wallet::TransactionKind::Debit,
            amount,
            description: description.clone(),
            sender: sender_name.clone(),
            receiver: receiver_name.clone(),
            tx_ref: tx_ref.clone(),
            timestamp: timestamp.clone(),
            lightning_payment_hash: lightning_payment_hash.clone(),
            extra: Default::default(),
        };

        // Build credit entry (for receiver's contract)
        let credit = cream_common::wallet::WalletTransaction {
            id: 0,
            kind: cream_common::wallet::TransactionKind::Credit,
            amount,
            description,
            sender: sender_name.clone(),
            receiver: receiver_name.clone(),
            tx_ref: tx_ref.clone(),
            timestamp,
            lightning_payment_hash,
            extra: Default::default(),
        };

        // Resolve sender key
        let sender_key = match &sender {
            ContractRole::Root => Some(*root_contract_key),
            ContractRole::User => user_contract_key.copied(),
            ContractRole::ThirdParty(key) => Some(*key),
        };
        if let Some(key) = sender_key {
            update_contract_ledger(api, shared, &sender, key, debit, signing_service).await;
        } else {
            clog("[CREAM] WARNING: sender contract key not available");
        }

        // Resolve receiver key
        let receiver_key = match &receiver {
            ContractRole::Root => Some(*root_contract_key),
            ContractRole::User => user_contract_key.copied(),
            ContractRole::ThirdParty(key) => Some(*key),
        };
        if let Some(key) = receiver_key {
            update_contract_ledger(api, shared, &receiver, key, credit, signing_service).await;
        } else {
            clog("[CREAM] WARNING: receiver contract key not available");
        }

        clog(&format!("[CREAM] Transfer recorded: {} CURD from {} to {} (tx_ref={})",
            amount, sender_name, receiver_name, tx_ref));
    }

    /// Append a transaction entry to a user contract and push the update to the network.
    async fn update_contract_ledger(
        api: &mut freenet_stdlib::client_api::WebApi,
        shared: &mut Signal<crate::components::shared_state::SharedState>,
        role: &ContractRole,
        contract_key: ContractKey,
        tx: cream_common::wallet::WalletTransaction,
        signing_service: &crate::components::signing_service::SigningService,
    ) {
        // ThirdParty: construct a minimal state with just the transaction entry.
        // The merge logic does ledger union unconditionally, so the credit gets
        // appended without overwriting the target's metadata. No network GET needed.
        if matches!(role, ContractRole::ThirdParty(_)) {
            // Use a dummy key for the minimal state — credit-only updates
            // are accepted without signature verification
            let dummy_key = ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]);
            let minimal_state = UserContractState {
                owner: cream_common::identity::UserId(dummy_key.verifying_key()),
                name: String::new(),
                origin_supplier: String::new(),
                current_supplier: String::new(),
                balance_curds: 0,
                invited_by: String::new(),
                toll_rates: Default::default(),
                checkpoint_balance: 0,
                checkpoint_tx_count: 0,
                checkpoint_at: None,
                pruned_lightning_hashes: Default::default(),
                ledger: vec![tx],
                next_tx_id: 0,
                updated_at: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                extra: Default::default(),
            };
            let uc_bytes = serde_json::to_vec(&minimal_state).unwrap();
            let update = ClientRequest::ContractOp(ContractRequest::Update {
                key: contract_key,
                data: UpdateData::State(State::from(uc_bytes)),
            });
            if let Err(e) = api.send(update).await {
                clog(&format!("[CREAM] ERROR: Failed to update third-party contract: {:?}", e));
            }
            return;
        }

        // Find the contract state in SharedState
        let mut uc_state = {
            let state = shared.read();
            match role {
                ContractRole::Root => state.root_user_contract.clone(),
                ContractRole::User => state.user_contract.clone(),
                ContractRole::ThirdParty(_) => unreachable!(),
            }
        };

        if let Some(ref mut uc) = uc_state {
            uc.ledger.push(tx);
            uc.balance_curds = uc.derive_balance();
            uc.next_tx_id = uc.ledger.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            uc.updated_at = chrono::Utc::now();
            uc.signature = match role {
                ContractRole::Root => {
                    signing_service.sign(&uc.signable_bytes()).await
                        .unwrap_or_else(|e| {
                            clog(&format!("[CREAM] ERROR: FROST signing failed: {}", e));
                            ed25519_dalek::Signature::from_bytes(&[0u8; 64])
                        })
                }
                _ => ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
            };

            let uc_bytes = serde_json::to_vec(&uc).unwrap();
            let update = ClientRequest::ContractOp(ContractRequest::Update {
                key: contract_key,
                data: UpdateData::State(State::from(uc_bytes)),
            });

            // Update SharedState
            {
                let mut state = shared.write();
                match role {
                    ContractRole::Root => state.root_user_contract = Some(uc.clone()),
                    ContractRole::User => state.user_contract = Some(uc.clone()),
                    ContractRole::ThirdParty(_) => unreachable!(),
                }
            }

            if let Err(e) = api.send(update).await {
                clog(&format!("[CREAM] ERROR: Failed to update contract: {:?}", e));
            }
        } else {
            clog("[CREAM] WARNING: No state found for contract");
        }
    }

    /// Convert a UI action into contract operations and send them.
    async fn handle_action(
        action: NodeAction,
        api: &mut freenet_stdlib::client_api::WebApi,
        shared: &mut Signal<crate::components::shared_state::SharedState>,
        directory_key: &ContractKey,
        sf_contract_keys: &mut BTreeMap<String, ContractKey>,
        key_manager: &KeyManager,
        #[allow(unused_variables)] node_url: &str,
        is_customer: bool,
        user_state: &Signal<crate::components::user_state::UserState>,
        send_half: &mpsc::UnboundedSender<ClientRequest<'static>>,
        user_contract_instance_id: &mut Option<ContractInstanceId>,
        user_contract_key_ref: &mut Option<ContractKey>,
        root_contract_key: &ContractKey,
        signing_service: &crate::components::signing_service::SigningService,
        inbox_contract_instance_id: &mut Option<ContractInstanceId>,
        inbox_contract_key_ref: &mut Option<ContractKey>,
        toll_rates: &Signal<cream_common::tolls::TollRates>,
        market_directory_key: &ContractKey,
    ) {
        // Construct wallet backend for this action dispatch
        let mut wallet = CreamNativeWallet::new(
            *shared,
            *root_contract_key,
            *user_contract_key_ref,
            signing_service.clone(),
        );

        match action {
            NodeAction::RegisterSupplier {
                name,
                postcode,
                locality,
                description,
            } => {
                let supplier_id = key_manager.user_id();
                let owner_key = key_manager.verifying_key();

                // Check if this supplier already exists in the directory
                // (e.g. pre-populated by the test harness). If so, just
                // populate the local maps and skip the PUT/Update.
                let existing_entry = shared.read().directory.entries
                    .get(&supplier_id).cloned();

                if let Some(entry) = existing_entry {
                    clog(&format!("[CREAM] RegisterSupplier: {} already in directory, skipping PUT", name));
                    let sf_key = entry.storefront_key;
                    sf_contract_keys.insert(name.clone(), sf_key);
                    shared.write().storefront_keys
                        .insert(name.clone(), format!("{}", sf_key));

                    // GET + subscribe to our own storefront so SharedState is populated
                    let get_sf = ClientRequest::ContractOp(ContractRequest::Get {
                        key: *sf_key.id(),
                        return_contract_code: false,
                        subscribe: false,
                        blocking_subscribe: false,
                    });
                    if let Err(e) = api.send(get_sf).await {
                        clog(&format!("[CREAM] ERROR: Failed to GET own storefront: {:?}", e));
                    }
                    let sub_sf = ClientRequest::ContractOp(ContractRequest::Subscribe {
                        key: *sf_key.id(),
                        summary: None,
                    });
                    if let Err(e) = api.send(sub_sf).await {
                        clog(&format!("[CREAM] ERROR: Failed to subscribe to own storefront: {:?}", e));
                    }

                    // Deploy inbox contract if it doesn't exist yet.
                    // (The test harness pre-populates directory/storefront/user
                    // contracts but NOT inbox contracts, so for harness-created
                    // users the inbox must be created on first UI login.)
                    if inbox_contract_instance_id.is_none() {
                        let owner_key = key_manager.verifying_key();
                        let inbox_params = cream_common::inbox::InboxParameters { owner: owner_key };
                        let params_bytes = serde_json::to_vec(&inbox_params).unwrap();
                        let inbox_contract = make_contract(INBOX_CONTRACT_WASM, Parameters::from(params_bytes));
                        let ib_key = inbox_contract.key();
                        let ib_state = cream_common::inbox::InboxState {
                            owner: key_manager.user_id(),
                            messages: std::collections::BTreeMap::new(),
                            updated_at: chrono::Utc::now(),
                            extra: Default::default(),
                        };
                        let ib_state_bytes = serde_json::to_vec(&ib_state).unwrap();
                        let put_inbox = ClientRequest::ContractOp(ContractRequest::Put {
                            contract: inbox_contract,
                            state: WrappedState::new(ib_state_bytes),
                            related_contracts: RelatedContracts::default(),
                            subscribe: true,
                            blocking_subscribe: false,
                        });
                        clog(&format!("[CREAM] Deploying inbox contract for existing user {}: {:?}", name, ib_key));
                        if let Err(e) = api.send(put_inbox).await {
                            clog(&format!("[CREAM] ERROR: Failed to deploy inbox contract: {:?}", e));
                        }
                        *inbox_contract_instance_id = Some(*ib_key.id());
                        *inbox_contract_key_ref = Some(ib_key);
                        {
                            let mut state = shared.write();
                            state.inbox = Some(ib_state);
                            state.inbox_contract_key = Some(format!("{}", ib_key));
                        }
                    }

                    // Still register with rendezvous service so customers can discover us
                    if !is_customer {
                        let rendezvous_name = name.to_lowercase().replace(' ', "-");
                        let node_address = node_url.to_string();
                        let sf_key_str = format!("{}", sf_key);
                        let uc_key_str = entry.user_contract_key.as_ref().map(|k| format!("{}", k));
                        let ib_key_str = inbox_contract_key_ref.as_ref().map(|k| format!("{}", k));
                        let pub_key_bytes = key_manager.verifying_key().as_bytes().to_vec();
                        let pub_key_hex: String = pub_key_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                        let sign_msg = format!("{}|{}|{}|{}|{}", rendezvous_name, node_address, sf_key_str, uc_key_str.as_deref().unwrap_or(""), ib_key_str.as_deref().unwrap_or(""));
                        let sig_bytes = key_manager.sign_raw(sign_msg.as_bytes());
                        let sig_hex: String = sig_bytes.iter().map(|b| format!("{:02x}", b)).collect();

                        let rname = rendezvous_name.clone();
                        let raddr = node_address.clone();
                        let reg_pub_hex = pub_key_hex.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match crate::components::rendezvous::register_supplier(
                                &rname, &raddr, &sf_key_str, &reg_pub_hex, &sig_hex,
                                uc_key_str.as_deref(), ib_key_str.as_deref(),
                            ).await {
                                Ok(()) => clog(&format!("[CREAM] Registered with rendezvous as '{}'", rname)),
                                Err(e) => clog(&format!("[CREAM] WARNING: Rendezvous registration failed: {}", e)),
                            }
                        });
                    }

                    return;
                }

                clog(&format!("[CREAM] RegisterSupplier: {} not found in directory (supplier_id={:?}), deploying NEW storefront. \
                    Note: for harness data, password must be the lowercase name (e.g. \"gary\")", name, supplier_id));

                // Look up postcode (+ locality if available) to get coordinates
                let location = locality
                    .as_deref()
                    .and_then(|loc| cream_common::postcode::lookup_locality(&postcode, loc))
                    .map(|info| info.location)
                    .or_else(|| cream_common::postcode::lookup_postcode(&postcode))
                    .unwrap_or(GeoLocation::new(-33.87, 151.21)); // Default to Sydney

                let storefront_params = StorefrontParameters { owner: owner_key };
                let params_bytes = serde_json::to_vec(&storefront_params).unwrap();
                let sf_contract = make_contract(
                    STOREFRONT_CONTRACT_WASM,
                    Parameters::from(params_bytes),
                );
                let sf_key = sf_contract.key();

                // Create initial storefront state
                let sf_state = StorefrontState {
                    info: StorefrontInfo {
                        owner: supplier_id.clone(),
                        name: name.clone(),
                        description: description.clone(),
                        location: location.clone(),
                        schedule: None,
                        timezone: None,
                        phone: None,
                        email: None,
                        address: None,
                        market_products: BTreeMap::new(),
                        extra: Default::default(),
                    },
                    products: BTreeMap::new(),
                    orders: BTreeMap::new(),
                    extra: Default::default(),
                };
                let sf_state_bytes = serde_json::to_vec(&sf_state).unwrap();

                // PUT the storefront contract
                let put_sf = ClientRequest::ContractOp(ContractRequest::Put {
                    contract: sf_contract,
                    state: WrappedState::new(sf_state_bytes),
                    related_contracts: RelatedContracts::default(),
                    subscribe: true,
                    blocking_subscribe: false,
                });

                clog(&format!("[CREAM] Deploying storefront for {}: {:?}", name, sf_key));
                if let Err(e) = api.send(put_sf).await {
                    clog(&format!("[CREAM] ERROR: Failed to deploy storefront: {:?}", e));
                    return;
                }

                // Store the storefront key and initial state
                sf_contract_keys.insert(name.clone(), sf_key);
                {
                    let mut state = shared.write();
                    state.storefront_keys
                        .insert(name.clone(), format!("{}", sf_contract_keys[&name]));
                    state.storefronts
                        .insert(name.clone(), sf_state);
                }

                // Deploy a user contract for the supplier (same pattern as customer RegisterUser)
                let supplier_uc_params = UserContractParameters { owner: owner_key };
                let supplier_uc_params_bytes = serde_json::to_vec(&supplier_uc_params).unwrap();
                let supplier_uc_contract = make_contract(
                    USER_CONTRACT_WASM,
                    Parameters::from(supplier_uc_params_bytes),
                );
                let supplier_uc_key = supplier_uc_contract.key();

                let supplier_uc_state = UserContractState {
                    owner: cream_common::identity::UserId(owner_key),
                    name: name.clone(),
                    origin_supplier: name.clone(),
                    current_supplier: name.clone(),
                    balance_curds: 0,
                    invited_by: String::new(),
                    toll_rates: Default::default(),
                    checkpoint_balance: 0,
                    checkpoint_tx_count: 0,
                    checkpoint_at: None,
                    pruned_lightning_hashes: Default::default(),
                    ledger: Vec::new(),
                    next_tx_id: 0,
                    updated_at: chrono::Utc::now(),
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    extra: Default::default(),
                };
                let supplier_uc_state_bytes = serde_json::to_vec(&supplier_uc_state).unwrap();

                let put_supplier_uc = ClientRequest::ContractOp(ContractRequest::Put {
                    contract: supplier_uc_contract,
                    state: WrappedState::new(supplier_uc_state_bytes),
                    related_contracts: RelatedContracts::default(),
                    subscribe: false,
                    blocking_subscribe: false,
                });

                clog(&format!("[CREAM] Deploying supplier user contract for {}: {:?}", name, supplier_uc_key));
                if let Err(e) = api.send(put_supplier_uc).await {
                    clog(&format!("[CREAM] ERROR: Failed to deploy supplier user contract: {:?}", e));
                }

                // Store supplier user contract key
                let supplier_uc_key_str = format!("{}", supplier_uc_key);
                shared.write().supplier_user_contract_key = Some(supplier_uc_key_str);

                // Transfer initial 10,000 CURD from root → supplier.
                // Use a deterministic tx_ref so re-registration deduplicates.
                wallet.transfer_from_root_to_third_party_idempotent(
                    api,
                    supplier_uc_key,
                    10_000,
                    "Initial CURD allocation".to_string(),
                    name.clone(),
                    format!("genesis:{}", name),
                ).await;

                // Now register in the directory with a real signature
                let mut entry = DirectoryEntry {
                    supplier: supplier_id,
                    name: name.clone(),
                    description,
                    location,
                    postcode: Some(postcode),
                    locality,
                    categories: vec![],
                    storefront_key: sf_key,
                    user_contract_key: Some(supplier_uc_key),
                    inbox_contract_key: inbox_contract_key_ref.clone(),
                    updated_at: chrono::Utc::now(),
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    extra: Default::default(),
                };
                key_manager.sign_directory_entry(&mut entry);

                // Update local state immediately so this tab sees its own supplier
                shared
                    .write()
                    .directory
                    .entries
                    .insert(entry.supplier.clone(), entry.clone());

                let mut entries = BTreeMap::new();
                entries.insert(entry.supplier.clone(), entry);
                let dir_update = DirectoryState { entries, extra: Default::default() };
                let delta_bytes = serde_json::to_vec(&dir_update).unwrap();

                let update_dir =
                    ClientRequest::ContractOp(ContractRequest::Update {
                        key: *directory_key,
                        data: UpdateData::Delta(StateDelta::from(delta_bytes)),
                    });

                clog(&format!("[CREAM] Registering {} in directory", name));
                if let Err(e) = api.send(update_dir).await {
                    clog(&format!("[CREAM] ERROR: Failed to update directory: {:?}", e));
                }

                // Register with rendezvous service (supplier mode only)
                if !is_customer {
                    let rendezvous_name = name.to_lowercase().replace(' ', "-");
                    let node_address = node_url.to_string();
                    let sf_key_str = format!("{}", sf_key);
                    let uc_key_str = Some(format!("{}", supplier_uc_key));
                    let ib_key_str = inbox_contract_key_ref.as_ref().map(|k| format!("{}", k));
                    let pub_key_bytes = key_manager.verifying_key().as_bytes().to_vec();
                    let pub_key_hex: String = pub_key_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                    let sign_msg = format!("{}|{}|{}|{}|{}", rendezvous_name, node_address, sf_key_str, uc_key_str.as_deref().unwrap_or(""), ib_key_str.as_deref().unwrap_or(""));
                    let sig_bytes = key_manager.sign_raw(sign_msg.as_bytes());
                    let sig_hex: String = sig_bytes.iter().map(|b| format!("{:02x}", b)).collect();

                    let rname = rendezvous_name.clone();
                    let raddr = node_address.clone();
                    let reg_pub_hex = pub_key_hex.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        match crate::components::rendezvous::register_supplier(
                            &rname, &raddr, &sf_key_str, &reg_pub_hex, &sig_hex,
                            uc_key_str.as_deref(), ib_key_str.as_deref(),
                        ).await {
                            Ok(()) => clog(&format!("[CREAM] Registered with rendezvous as '{}'", rname)),
                            Err(e) => clog(&format!("[CREAM] WARNING: Rendezvous registration failed: {}", e)),
                        }
                    });

                    // Start background heartbeat task (every 5 minutes)
                    let hb_name = rendezvous_name;
                    let hb_addr = node_address;
                    let hb_pub_hex = pub_key_hex;
                    let hb_km = key_manager.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        loop {
                            gloo_timers_sleep(5 * 60 * 1000).await;
                            let sign_msg = format!("{}|{}", hb_name, hb_addr);
                            let sig_bytes = hb_km.sign_raw(sign_msg.as_bytes());
                            let sig_hex: String = sig_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                            match crate::components::rendezvous::heartbeat(
                                &hb_name, &hb_addr, &hb_pub_hex, &sig_hex,
                            ).await {
                                Ok(()) => clog("[CREAM] Heartbeat sent"),
                                Err(e) => clog(&format!("[CREAM] WARNING: Heartbeat failed: {}", e)),
                            }
                        }
                    });

                    // Start background order-expiry task (checks once per hour,
                    // expires Reserved orders whose hold date has passed).
                    let expiry_supplier = name.clone();
                    let mut expiry_shared = shared.clone();
                    let expiry_sf_keys = sf_contract_keys.clone();
                    let mut expiry_sender = send_half.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let mut last_run_date: Option<chrono::NaiveDate> = None;
                        loop {
                            gloo_timers_sleep(60 * 60 * 1000).await; // 1 hour
                            let today = chrono::Utc::now().date_naive();
                            if last_run_date == Some(today) {
                                continue;
                            }
                            last_run_date = Some(today);

                            // Clone the storefront, run expiry, and update if changed
                            let sf_opt = expiry_shared.read().storefronts
                                .get(&expiry_supplier).cloned();
                            if let Some(mut sf) = sf_opt {
                                let now = chrono::Utc::now();
                                let orders_changed = sf.expire_orders(now);
                                if orders_changed {
                                    clog(&format!("[CREAM] Expired orders for '{}'", expiry_supplier));
                                    expiry_shared.write().storefronts
                                        .insert(expiry_supplier.clone(), sf.clone());
                                    // Push to network via the internal request channel
                                    if let Some(sf_key) = expiry_sf_keys.get(&expiry_supplier) {
                                        let sf_bytes = serde_json::to_vec(&sf).unwrap();
                                        let update = ClientRequest::ContractOp(
                                            ContractRequest::Update {
                                                key: *sf_key,
                                                data: UpdateData::State(
                                                    State::from(sf_bytes),
                                                ),
                                            },
                                        );
                                        if let Err(e) = expiry_sender.send(update).await {
                                            clog(&format!("[CREAM] ERROR: Failed to send expiry update: {:?}", e));
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
            }

            NodeAction::DeployStorefront { .. } => {
                // Handled as part of RegisterSupplier above
                tracing::debug!("DeployStorefront handled via RegisterSupplier");
            }

            NodeAction::AddProduct {
                name,
                category,
                description,
                price_curd,
                quantity_total,
            } => {
                // Find this user's storefront key from the directory using their UserId.
                // This works whether the storefront was deployed by this tab (RegisterSupplier)
                // or pre-populated by the test harness / another session.
                let my_supplier_id = key_manager.user_id();
                clog(&format!("[CREAM] AddProduct: looking up storefront for supplier {:?}", my_supplier_id));
                let (supplier_name, sf_key) = {
                    let state = shared.read();
                    clog(&format!("[CREAM] AddProduct: sf_contract_keys has {} entries, directory has {} entries",
                        sf_contract_keys.len(), state.directory.entries.len()));
                    // Look up by our supplier ID in the directory (most reliable —
                    // sf_contract_keys contains ALL storefronts, not just ours)
                    let result = state.directory.entries.get(&my_supplier_id)
                        .map(|entry| (entry.name.clone(), entry.storefront_key))
                        .or_else(|| {
                            sf_contract_keys.iter().next()
                                .map(|(name, key)| (name.clone(), *key))
                        });
                    clog(&format!("[CREAM] AddProduct: found storefront = {}", result.is_some()));
                    result.unzip()
                };

                let (Some(supplier_name), Some(sf_key)) = (supplier_name, sf_key) else {
                    clog("[CREAM] ERROR: No storefront found for supplier, can't add product");
                    return;
                };
                clog(&format!("[CREAM] AddProduct: supplier_name={}, sf_key={:?}", supplier_name, sf_key));

                // Build a product update with the existing storefront state
                let now = chrono::Utc::now();
                let product_id = ProductId(format!("p-{}", now.timestamp_millis()));
                let cat = match category.as_str() {
                    "Milk" => ProductCategory::Milk,
                    "Cheese" => ProductCategory::Cheese,
                    "Butter" => ProductCategory::Butter,
                    "Cream" => ProductCategory::Cream,
                    "Yogurt" => ProductCategory::Yogurt,
                    "Kefir" => ProductCategory::Kefir,
                    other => ProductCategory::Other(other.to_string()),
                };
                let product = Product {
                    id: product_id.clone(),
                    name,
                    description,
                    category: cat,
                    price_curd,
                    quantity_total,
                    expiry_date: None,
                    updated_at: now,
                    created_at: now,
                    extra: Default::default(),
                };
                let signature = key_manager.sign_product(&product);
                let signed_product = SignedProduct {
                    product,
                    signature,
                    extra: Default::default(),
                };

                // Get the current storefront state and add the product
                let existing_sf = {
                    shared
                        .read()
                        .storefronts
                        .get(&supplier_name)
                        .cloned()
                };

                if let Some(mut sf) = existing_sf {
                    sf.products.insert(product_id.clone(), signed_product.clone());
                    let sf_bytes = serde_json::to_vec(&sf).unwrap();
                    clog(&format!("[CREAM] AddProduct: sending Update with {} products, {} bytes",
                        sf.products.len(), sf_bytes.len()));

                    let update =
                        ClientRequest::ContractOp(ContractRequest::Update {
                            key: sf_key,
                            data: UpdateData::State(State::from(sf_bytes)),
                        });

                    // Update local SharedState immediately so the supplier sees their product
                    shared.write().storefronts.insert(supplier_name.clone(), sf);

                    if let Err(e) = api.send(update).await {
                        clog(&format!("[CREAM] ERROR: Failed to add product: {:?}", e));
                    } else {
                        clog("[CREAM] AddProduct: Update sent successfully");
                    }
                } else {
                    clog(&format!("[CREAM] ERROR: Storefront state not found for {}", supplier_name));
                }
            }

            NodeAction::RemoveProduct { product_id } => {
                tracing::debug!("RemoveProduct: {} (not yet implemented)", product_id);
            }

            NodeAction::PlaceOrder {
                storefront_name,
                product_id,
                quantity,
                deposit_tier,
                price_per_unit,
                collection_point,
            } => {
                clog(&format!("[CREAM] PlaceOrder on {}: {} x{} ({})",
                    storefront_name, product_id, quantity, deposit_tier));

                // Find the storefront's contract key.
                // Check sf_contract_keys first (populated from GetResponse),
                // then fall back to directory entries.
                let sf_key = sf_contract_keys.get(&storefront_name).copied()
                    .or_else(|| sf_contract_keys.values().next().copied())
                    .or_else(|| {
                        let state = shared.read();
                        state.directory.entries.values()
                            .find(|e| e.name == storefront_name)
                            .map(|e| e.storefront_key)
                    });

                let Some(sf_key) = sf_key else {
                    clog(&format!("[CREAM] ERROR: No storefront key found for {}, can't place order", storefront_name));
                    return;
                };

                // Get the existing storefront state
                let existing_sf = {
                    shared.read().storefronts.get(&storefront_name).cloned()
                };

                let Some(mut sf) = existing_sf else {
                    clog(&format!("[CREAM] ERROR: Storefront state not found for {}", storefront_name));
                    return;
                };

                // Parse deposit tier
                let tier = match deposit_tier.as_str() {
                    "2-Day Reserve (10%)" => DepositTier::Reserve2Days,
                    "1-Week Reserve (20%)" => DepositTier::Reserve1Week,
                    "Full Payment (100%)" => DepositTier::FullPayment,
                    _ => {
                        clog(&format!("[CREAM] ERROR: Unknown deposit tier: {}", deposit_tier));
                        return;
                    }
                };

                // Calculate pricing
                let now = chrono::Utc::now();
                let total_price = price_per_unit * quantity as u64;
                let deposit_amount = tier.calculate_deposit(total_price);

                // Calculate reservation expiry
                let expires_at = match tier {
                    DepositTier::Reserve2Days => now + chrono::Duration::days(2),
                    DepositTier::Reserve1Week => now + chrono::Duration::weeks(1),
                    DepositTier::FullPayment => now + chrono::Duration::days(365),
                };

                // Build the order
                let order_id = OrderId(format!("o-{}", now.timestamp_millis()));
                let mut order = Order {
                    id: order_id.clone(),
                    product_id: ProductId(product_id),
                    customer: key_manager.user_id(),
                    quantity,
                    deposit_tier: tier,
                    deposit_amount,
                    total_price,
                    status: OrderStatus::Reserved { expires_at },
                    created_at: now,
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    escrow_token: None,
                    collection_point,
                    extra: Default::default(),
                };

                // Sign the order with the customer key
                key_manager.sign_order(&mut order);

                // Insert into storefront and send update
                sf.orders.insert(order_id.clone(), order);

                let sf_bytes = serde_json::to_vec(&sf).unwrap();
                clog(&format!("[CREAM] PlaceOrder: sending Update with {} orders, {} bytes",
                    sf.orders.len(), sf_bytes.len()));

                let update = ClientRequest::ContractOp(ContractRequest::Update {
                    key: sf_key,
                    data: UpdateData::State(State::from(sf_bytes)),
                });

                // Update local SharedState immediately
                shared.write().storefronts.insert(storefront_name.clone(), sf);

                if let Err(e) = api.send(update).await {
                    clog(&format!("[CREAM] ERROR: Failed to place order: {:?}", e));
                } else {
                    clog("[CREAM] PlaceOrder: Update sent successfully");

                    // Record double-entry transfer: customer → root (deposit)
                    let customer_name = user_state.read().moniker.clone().unwrap_or_default();
                    wallet.transfer_to_root(
                        api,
                        deposit_amount,
                        format!("Order deposit: {}", storefront_name),
                        customer_name,
                    ).await;
                }
            }

            NodeAction::SubscribeStorefront { supplier_name } => {
                // Look up the storefront's contract key from the directory
                let sf_key = {
                    let state = shared.read();
                    state
                        .directory
                        .entries
                        .values()
                        .find(|e| e.name == supplier_name)
                        .map(|e| *e.storefront_key.id())
                };

                if let Some(instance_id) = sf_key {
                    let subscribe =
                        ClientRequest::ContractOp(ContractRequest::Get {
                            key: instance_id,
                            return_contract_code: false,
                            subscribe: true,
                            blocking_subscribe: false,
                        });

                    tracing::info!(
                        "Subscribing to storefront for {}",
                        supplier_name
                    );
                    if let Err(e) = api.send(subscribe).await {
                        tracing::error!("Failed to subscribe to storefront: {:?}", e);
                    }
                } else {
                    tracing::warn!(
                        "No directory entry found for {}, can't subscribe",
                        supplier_name
                    );
                }
            }

            NodeAction::UpdateSchedule { schedule } => {
                clog("[CREAM] UpdateSchedule: updating opening hours");
                let my_supplier_id = key_manager.user_id();
                let (supplier_name, sf_key) = {
                    let state = shared.read();
                    // Look up by our supplier ID in the directory (most reliable —
                    // sf_contract_keys contains ALL storefronts, not just ours)
                    state
                        .directory
                        .entries
                        .get(&my_supplier_id)
                        .map(|entry| (entry.name.clone(), entry.storefront_key))
                        .or_else(|| {
                            sf_contract_keys
                                .iter()
                                .next()
                                .map(|(name, key)| (name.clone(), *key))
                        })
                        .unzip()
                };

                let (Some(supplier_name), Some(sf_key)) = (supplier_name, sf_key) else {
                    clog("[CREAM] ERROR: No storefront found, can't update schedule");
                    return;
                };

                let existing_sf = shared.read().storefronts.get(&supplier_name).cloned();
                if let Some(mut sf) = existing_sf {
                    // Derive timezone from postcode
                    let tz = {
                        let us = user_state.read();
                        us.postcode
                            .as_deref()
                            .and_then(cream_common::postcode::timezone_for_postcode)
                    };
                    sf.info.schedule = Some(schedule);
                    sf.info.timezone = tz;

                    let sf_bytes = serde_json::to_vec(&sf).unwrap();
                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key: sf_key,
                        data: UpdateData::State(State::from(sf_bytes)),
                    });
                    shared
                        .write()
                        .storefronts
                        .insert(supplier_name.clone(), sf);

                    if let Err(e) = api.send(update).await {
                        clog(&format!(
                            "[CREAM] ERROR: Failed to update schedule: {:?}",
                            e
                        ));
                    } else {
                        clog("[CREAM] UpdateSchedule: sent successfully");
                    }
                } else {
                    clog(&format!(
                        "[CREAM] ERROR: Storefront state not found for {}",
                        supplier_name
                    ));
                }
            }

            NodeAction::CancelOrder { order_id } => {
                clog(&format!("[CREAM] CancelOrder: {}", order_id));
                let my_supplier_id = key_manager.user_id();
                let (supplier_name, sf_key) = {
                    let state = shared.read();
                    state
                        .directory
                        .entries
                        .get(&my_supplier_id)
                        .map(|entry| (entry.name.clone(), entry.storefront_key))
                        .or_else(|| {
                            sf_contract_keys
                                .iter()
                                .next()
                                .map(|(name, key)| (name.clone(), *key))
                        })
                        .unzip()
                };

                let (Some(supplier_name), Some(sf_key)) = (supplier_name, sf_key) else {
                    clog("[CREAM] ERROR: No storefront found, can't cancel order");
                    return;
                };

                let existing_sf = shared.read().storefronts.get(&supplier_name).cloned();
                if let Some(mut sf) = existing_sf {
                    let oid = OrderId(order_id.clone());
                    if let Some(order) = sf.orders.get_mut(&oid) {
                        if !order.status.can_transition_to(&OrderStatus::Cancelled) {
                            clog(&format!(
                                "[CREAM] ERROR: Cannot cancel order {} in status {}",
                                order_id, order.status
                            ));
                            return;
                        }

                        // Capture refund info before mutating
                        let deposit_amount = order.deposit_amount;
                        let customer_vk = order.customer.0;

                        order.status = OrderStatus::Cancelled;

                        let sf_bytes = serde_json::to_vec(&sf).unwrap();
                        let update = ClientRequest::ContractOp(ContractRequest::Update {
                            key: sf_key,
                            data: UpdateData::State(State::from(sf_bytes)),
                        });
                        shared
                            .write()
                            .storefronts
                            .insert(supplier_name.clone(), sf);

                        if let Err(e) = api.send(update).await {
                            clog(&format!(
                                "[CREAM] ERROR: Failed to cancel order: {:?}",
                                e
                            ));
                        } else {
                            clog("[CREAM] CancelOrder: sent successfully");
                        }

                        // Refund escrow deposit: root → customer's user contract
                        if deposit_amount > 0 {
                            let customer_uc_params = UserContractParameters { owner: customer_vk };
                            let customer_params_bytes = serde_json::to_vec(&customer_uc_params).unwrap();
                            let customer_uc_contract = make_contract(
                                USER_CONTRACT_WASM,
                                Parameters::from(customer_params_bytes),
                            );
                            let customer_uc_key = customer_uc_contract.key();

                            wallet.transfer_from_root_to_third_party(
                                api,
                                customer_uc_key,
                                deposit_amount,
                                format!("Escrow refund: cancelled order {}", order_id),
                                "customer".to_string(),
                            ).await;

                            clog(&format!(
                                "[CREAM] CancelOrder: refunded {} CURD to customer",
                                deposit_amount
                            ));
                        }
                    } else {
                        clog(&format!(
                            "[CREAM] ERROR: Order {} not found in storefront",
                            order_id
                        ));
                    }
                } else {
                    clog(&format!(
                        "[CREAM] ERROR: Storefront state not found for {}",
                        supplier_name
                    ));
                }
            }

            NodeAction::FulfillOrder { order_id } => {
                clog(&format!("[CREAM] FulfillOrder: {}", order_id));
                let my_supplier_id = key_manager.user_id();
                let (supplier_name, sf_key) = {
                    let state = shared.read();
                    state
                        .directory
                        .entries
                        .get(&my_supplier_id)
                        .map(|entry| (entry.name.clone(), entry.storefront_key))
                        .or_else(|| {
                            sf_contract_keys
                                .iter()
                                .next()
                                .map(|(name, key)| (name.clone(), *key))
                        })
                        .unzip()
                };

                let (Some(supplier_name), Some(sf_key)) = (supplier_name, sf_key) else {
                    clog("[CREAM] ERROR: No storefront found, can't fulfill order");
                    return;
                };

                let existing_sf = shared.read().storefronts.get(&supplier_name).cloned();
                if let Some(mut sf) = existing_sf {
                    let oid = OrderId(order_id.clone());
                    if let Some(order) = sf.orders.get_mut(&oid) {
                        if !order.status.can_transition_to(&OrderStatus::Fulfilled) {
                            clog(&format!(
                                "[CREAM] ERROR: Cannot fulfill order {} in status {}",
                                order_id, order.status
                            ));
                            return;
                        }
                        let deposit_amount = order.deposit_amount;
                        order.status = OrderStatus::Fulfilled;

                        let sf_bytes = serde_json::to_vec(&sf).unwrap();
                        let update = ClientRequest::ContractOp(ContractRequest::Update {
                            key: sf_key,
                            data: UpdateData::State(State::from(sf_bytes)),
                        });
                        shared
                            .write()
                            .storefronts
                            .insert(supplier_name.clone(), sf);

                        if let Err(e) = api.send(update).await {
                            clog(&format!(
                                "[CREAM] ERROR: Failed to fulfill order: {:?}",
                                e
                            ));
                        } else {
                            clog("[CREAM] FulfillOrder: sent successfully");
                        }

                        // Settle escrow: transfer deposit from root → supplier's user contract
                        let supplier_uc_key = shared.read().directory.entries
                            .get(&my_supplier_id)
                            .and_then(|entry| entry.user_contract_key);

                        if let Some(uc_key) = supplier_uc_key {
                            wallet.settle_escrow_to_supplier(
                                api,
                                uc_key,
                                deposit_amount,
                                format!("Escrow settlement for order {}", order_id),
                                supplier_name.clone(),
                            ).await;
                            clog(&format!(
                                "[CREAM] FulfillOrder: settled {} CURD escrow to {}",
                                deposit_amount, supplier_name
                            ));
                        } else {
                            clog("[CREAM] WARNING: No supplier user contract key, escrow not settled");
                        }
                    } else {
                        clog(&format!(
                            "[CREAM] ERROR: Order {} not found in storefront",
                            order_id
                        ));
                    }
                } else {
                    clog(&format!(
                        "[CREAM] ERROR: Storefront state not found for {}",
                        supplier_name
                    ));
                }
            }

            NodeAction::UpdateProduct {
                product_id,
                price_curd,
                quantity_total,
            } => {
                clog(&format!(
                    "[CREAM] UpdateProduct: {} price={} qty={}",
                    product_id, price_curd, quantity_total
                ));
                let my_supplier_id = key_manager.user_id();
                let (supplier_name, sf_key) = {
                    let state = shared.read();
                    state
                        .directory
                        .entries
                        .get(&my_supplier_id)
                        .map(|entry| (entry.name.clone(), entry.storefront_key))
                        .or_else(|| {
                            sf_contract_keys
                                .iter()
                                .next()
                                .map(|(name, key)| (name.clone(), *key))
                        })
                        .unzip()
                };

                let (Some(supplier_name), Some(sf_key)) = (supplier_name, sf_key) else {
                    clog("[CREAM] ERROR: No storefront found, can't update product");
                    return;
                };

                let existing_sf = shared.read().storefronts.get(&supplier_name).cloned();
                if let Some(mut sf) = existing_sf {
                    let pid = ProductId(product_id.clone());
                    if let Some(signed_product) = sf.products.get_mut(&pid) {
                        signed_product.product.price_curd = price_curd;
                        signed_product.product.quantity_total = quantity_total;
                        signed_product.product.updated_at = chrono::Utc::now();
                        signed_product.signature =
                            key_manager.sign_product(&signed_product.product);

                        let sf_bytes = serde_json::to_vec(&sf).unwrap();
                        let update = ClientRequest::ContractOp(ContractRequest::Update {
                            key: sf_key,
                            data: UpdateData::State(State::from(sf_bytes)),
                        });
                        shared
                            .write()
                            .storefronts
                            .insert(supplier_name.clone(), sf);

                        if let Err(e) = api.send(update).await {
                            clog(&format!(
                                "[CREAM] ERROR: Failed to update product: {:?}",
                                e
                            ));
                        } else {
                            clog("[CREAM] UpdateProduct: sent successfully");
                        }
                    } else {
                        clog(&format!(
                            "[CREAM] ERROR: Product {} not found in storefront",
                            product_id
                        ));
                    }
                } else {
                    clog(&format!(
                        "[CREAM] ERROR: Storefront state not found for {}",
                        supplier_name
                    ));
                }
            }

            NodeAction::UpdateContactDetails {
                phone,
                email,
                address,
            } => {
                clog("[CREAM] UpdateContactDetails: updating contact info");
                let my_supplier_id = key_manager.user_id();
                let (supplier_name, sf_key) = {
                    let state = shared.read();
                    state
                        .directory
                        .entries
                        .get(&my_supplier_id)
                        .map(|entry| (entry.name.clone(), entry.storefront_key))
                        .or_else(|| {
                            sf_contract_keys
                                .iter()
                                .next()
                                .map(|(name, key)| (name.clone(), *key))
                        })
                        .unzip()
                };

                let (Some(supplier_name), Some(sf_key)) = (supplier_name, sf_key) else {
                    clog("[CREAM] ERROR: No storefront found, can't update contact details");
                    return;
                };

                let existing_sf = shared.read().storefronts.get(&supplier_name).cloned();
                if let Some(mut sf) = existing_sf {
                    sf.info.phone = phone;
                    sf.info.email = email;
                    sf.info.address = address;

                    let sf_bytes = serde_json::to_vec(&sf).unwrap();
                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key: sf_key,
                        data: UpdateData::State(State::from(sf_bytes)),
                    });
                    shared
                        .write()
                        .storefronts
                        .insert(supplier_name.clone(), sf);

                    if let Err(e) = api.send(update).await {
                        clog(&format!(
                            "[CREAM] ERROR: Failed to update contact details: {:?}",
                            e
                        ));
                    } else {
                        clog("[CREAM] UpdateContactDetails: sent successfully");
                    }
                } else {
                    clog(&format!(
                        "[CREAM] ERROR: Storefront state not found for {}",
                        supplier_name
                    ));
                }
            }

            NodeAction::RegisterUser {
                name,
                origin_supplier,
                current_supplier,
                invited_by,
            } => {
                clog(&format!("[CREAM] RegisterUser: {} (origin={}, current={}, invited_by={})",
                    name, origin_supplier, current_supplier, invited_by));

                let owner_key = key_manager.verifying_key();
                let uc_params = UserContractParameters { owner: owner_key };
                let params_bytes = serde_json::to_vec(&uc_params).unwrap();
                let uc_contract = make_contract(
                    USER_CONTRACT_WASM,
                    Parameters::from(params_bytes),
                );
                let uc_key = uc_contract.key();

                let now = chrono::Utc::now();
                let uc_state = UserContractState {
                    owner: key_manager.user_id(),
                    name: name.clone(),
                    origin_supplier,
                    current_supplier,
                    balance_curds: 0,
                    invited_by,
                    toll_rates: Default::default(),
                    checkpoint_balance: 0,
                    checkpoint_tx_count: 0,
                    checkpoint_at: None,
                    pruned_lightning_hashes: Default::default(),
                    ledger: Vec::new(),
                    next_tx_id: 0,
                    updated_at: now,
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    extra: Default::default(),
                };
                let uc_state_bytes = serde_json::to_vec(&uc_state).unwrap();

                let put_uc = ClientRequest::ContractOp(ContractRequest::Put {
                    contract: uc_contract,
                    state: WrappedState::new(uc_state_bytes),
                    related_contracts: RelatedContracts::default(),
                    subscribe: true,
                    blocking_subscribe: false,
                });

                clog(&format!("[CREAM] Deploying user contract for {}: {:?}", name, uc_key));
                if let Err(e) = api.send(put_uc).await {
                    clog(&format!("[CREAM] ERROR: Failed to deploy user contract: {:?}", e));
                    return;
                }

                // Store the user contract key
                let uc_key_str = format!("{}", uc_key);
                *user_contract_instance_id = Some(*uc_key.id());
                *user_contract_key_ref = Some(uc_key);
                {
                    let mut state = shared.write();
                    state.user_contract = Some(uc_state);
                    state.user_contract_key = Some(uc_key_str.clone());
                }
                // Persist to UserState for reload
                {
                    let mut us_signal = *user_state;
                    let mut us = us_signal.write();
                    us.user_contract_key = Some(uc_key_str);
                    us.save();
                }

                // Transfer initial 10,000 CURD from root → new user.
                // Use a deterministic tx_ref so re-registration deduplicates
                // (the ledger merge uses tx_ref+kind as the dedup key).
                wallet.user_contract_key = Some(uc_key);
                wallet.transfer_from_root_idempotent(
                    api,
                    10_000,
                    "Initial CURD allocation".to_string(),
                    name.clone(),
                    format!("genesis:{}", name),
                ).await;

                // Deploy inbox contract for this user
                let inbox_params = cream_common::inbox::InboxParameters {
                    owner: key_manager.verifying_key(),
                };
                let inbox_params_bytes = serde_json::to_vec(&inbox_params).unwrap();
                let inbox_contract = make_contract(
                    INBOX_CONTRACT_WASM,
                    Parameters::from(inbox_params_bytes),
                );
                let ib_key = inbox_contract.key();
                let ib_state = cream_common::inbox::InboxState {
                    owner: key_manager.user_id(),
                    messages: std::collections::BTreeMap::new(),
                    updated_at: now,
                    extra: Default::default(),
                };
                let ib_state_bytes = serde_json::to_vec(&ib_state).unwrap();
                let put_inbox = ClientRequest::ContractOp(ContractRequest::Put {
                    contract: inbox_contract,
                    state: WrappedState::new(ib_state_bytes),
                    related_contracts: RelatedContracts::default(),
                    subscribe: true,
                    blocking_subscribe: false,
                });

                clog(&format!("[CREAM] Deploying inbox contract for {}: {:?}", name, ib_key));
                if let Err(e) = api.send(put_inbox).await {
                    clog(&format!("[CREAM] ERROR: Failed to deploy inbox contract: {:?}", e));
                }
                *inbox_contract_instance_id = Some(*ib_key.id());
                *inbox_contract_key_ref = Some(ib_key);
                {
                    let mut state = shared.write();
                    state.inbox = Some(ib_state);
                    state.inbox_contract_key = Some(format!("{}", ib_key));
                }
            }

            NodeAction::UpdateUserContract {
                current_supplier,
            } => {
                let Some(uc_key) = *user_contract_key_ref else {
                    clog("[CREAM] UpdateUserContract: no user contract key, skipping");
                    return;
                };

                let existing = shared.read().user_contract.clone();
                if let Some(mut uc_state) = existing {
                    if let Some(supplier) = current_supplier {
                        uc_state.current_supplier = supplier;
                    }
                    uc_state.updated_at = chrono::Utc::now();
                    uc_state.balance_curds = uc_state.derive_balance();
                    // Re-sign (dev mode: signature is ignored)
                    uc_state.signature = ed25519_dalek::Signature::from_bytes(&[0u8; 64]);

                    let uc_bytes = serde_json::to_vec(&uc_state).unwrap();
                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key: uc_key,
                        data: UpdateData::State(State::from(uc_bytes)),
                    });
                    shared.write().user_contract = Some(uc_state);

                    if let Err(e) = api.send(update).await {
                        clog(&format!("[CREAM] ERROR: Failed to update user contract: {:?}", e));
                    } else {
                        clog("[CREAM] UpdateUserContract: sent successfully");
                    }
                } else {
                    clog("[CREAM] UpdateUserContract: no existing user contract state");
                }
            }

            NodeAction::PegIn { amount_sats } => {
                use cream_common::lightning_gateway::{LightningGateway, PaymentStatus};
                use super::super::lightning_mock::MockLightningGateway;
                let curd_per_sat = toll_rates.read().curd_per_sat;

                clog(&format!("[CREAM] PegIn: {} sats → {} CURD", amount_sats, amount_sats * curd_per_sat));
                let mut gw = MockLightningGateway::new();

                let invoice = match gw.create_invoice(amount_sats, "CURD peg-in") {
                    Ok(inv) => inv,
                    Err(e) => {
                        clog(&format!("[CREAM] ERROR: PegIn create_invoice failed: {}", e));
                        return;
                    }
                };

                match gw.check_invoice(&invoice.payment_hash) {
                    Ok(PaymentStatus::Success { .. }) => {
                        let curd_amount = amount_sats * curd_per_sat;
                        let user_name = user_state.read().moniker.clone().unwrap_or_default();
                        wallet.transfer_from_root(
                            api,
                            curd_amount,
                            format!("Lightning peg-in ({} sats)", amount_sats),
                            user_name,
                        ).await;
                        clog(&format!("[CREAM] PegIn: credited {} CURD", curd_amount));
                    }
                    Ok(_) => {
                        clog("[CREAM] ERROR: PegIn invoice not yet paid");
                    }
                    Err(e) => {
                        clog(&format!("[CREAM] ERROR: PegIn check_invoice failed: {}", e));
                    }
                }
            }

            NodeAction::PegInAllocate { amount_sats, payment_hash } => {
                let curd_amount = amount_sats * toll_rates.read().curd_per_sat;
                clog(&format!("[CREAM] PegInAllocate: {} sats → {} CURD (hash: {})", amount_sats, curd_amount, payment_hash));

                let user_name = user_state.read().moniker.clone().unwrap_or_default();
                wallet.transfer_from_root_with_lightning_hash(
                    api,
                    curd_amount,
                    format!("Lightning peg-in ({} sats)", amount_sats),
                    user_name,
                    payment_hash.clone(),
                ).await;
                clog(&format!("[CREAM] PegInAllocate: credited {} CURD, settling invoice", curd_amount));

                // Settle the hold invoice via the gateway
                if let Some(client) = super::super::lightning_remote::LightningClient::from_env() {
                    match client.settle_pegin(&payment_hash).await {
                        Ok(()) => clog("[CREAM] PegInAllocate: invoice settled"),
                        Err(e) => clog(&format!("[CREAM] ERROR: PegInAllocate settle failed: {}", e)),
                    }
                }
            }

            NodeAction::PegOut { amount_curd, bolt11 } => {
                use cream_common::lightning_gateway::{LightningGateway, PaymentStatus};
                use super::super::lightning_mock::MockLightningGateway;

                let sats_out = amount_curd / toll_rates.read().curd_per_sat;
                clog(&format!("[CREAM] PegOut: {} CURD → {} sats", amount_curd, sats_out));

                // Check balance
                let current_balance = shared.read().user_contract
                    .as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
                if current_balance < amount_curd {
                    clog(&format!(
                        "[CREAM] ERROR: PegOut insufficient balance: have {}, need {}",
                        current_balance, amount_curd
                    ));
                    return;
                }

                // Debit CURD from user → root
                let user_name = user_state.read().moniker.clone().unwrap_or_default();
                wallet.transfer_to_root(
                    api,
                    amount_curd,
                    format!("Lightning peg-out ({} sats)", sats_out),
                    user_name.clone(),
                ).await;

                // Pay the Lightning invoice
                let mut gw = MockLightningGateway::new();
                match gw.pay_invoice(&bolt11) {
                    Ok(PaymentStatus::Success { .. }) => {
                        clog(&format!("[CREAM] PegOut: paid {} sats to {}", sats_out, bolt11));
                    }
                    Ok(_) | Err(_) => {
                        // Refund on failure
                        clog("[CREAM] ERROR: PegOut Lightning payment failed, refunding");
                        wallet.transfer_from_root(
                            api,
                            amount_curd,
                            "Lightning peg-out refund (payment failed)".to_string(),
                            user_name,
                        ).await;
                    }
                }
            }

            NodeAction::PegOutViaGateway { amount_curd, bolt11 } => {
                let sats_out = amount_curd / toll_rates.read().curd_per_sat;
                clog(&format!("[CREAM] PegOutViaGateway: {} CURD → {} sats", amount_curd, sats_out));

                // Check balance
                let current_balance = shared.read().user_contract
                    .as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
                if current_balance < amount_curd {
                    clog(&format!(
                        "[CREAM] ERROR: PegOut insufficient balance: have {}, need {}",
                        current_balance, amount_curd
                    ));
                    return;
                }

                // Use a hash of the bolt11 as the lightning_payment_hash for dedup
                let pegout_hash = format!("pegout:{}", &bolt11[..bolt11.len().min(32)]);

                // Debit CURD from user → root
                let user_name = user_state.read().moniker.clone().unwrap_or_default();
                wallet.transfer_to_root_with_lightning_hash(
                    api,
                    amount_curd,
                    format!("Lightning peg-out ({} sats)", sats_out),
                    user_name.clone(),
                    pegout_hash,
                ).await;

                // Pay via gateway
                if let Some(client) = super::super::lightning_remote::LightningClient::from_env() {
                    match client.pay_invoice(&bolt11, sats_out).await {
                        Ok(resp) if resp.success => {
                            clog(&format!("[CREAM] PegOutViaGateway: paid {} sats", sats_out));
                        }
                        Ok(resp) => {
                            clog(&format!("[CREAM] ERROR: PegOutViaGateway payment failed: {:?}, refunding", resp.error));
                            wallet.transfer_from_root(
                                api,
                                amount_curd,
                                "Lightning peg-out refund (payment failed)".to_string(),
                                user_name,
                            ).await;
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: PegOutViaGateway request failed: {}, refunding", e));
                            wallet.transfer_from_root(
                                api,
                                amount_curd,
                                "Lightning peg-out refund (gateway error)".to_string(),
                                user_name,
                            ).await;
                        }
                    }
                } else {
                    clog("[CREAM] ERROR: PegOutViaGateway: no gateway configured, refunding");
                    wallet.transfer_from_root(
                        api,
                        amount_curd,
                        "Lightning peg-out refund (no gateway)".to_string(),
                        user_name,
                    ).await;
                }
            }

            NodeAction::FaucetTopUp => {
                clog("[CREAM] FaucetTopUp: transferring 1000 CURD from root");
                let user_name = user_state.read().moniker.clone().unwrap_or_default();
                wallet.transfer_from_root(
                    api,
                    1_000,
                    "Faucet".to_string(),
                    user_name,
                ).await;
            }

            NodeAction::SendInboxMessage {
                recipient_name,
                body,
                kind,
                recipient_pubkey_hex,
            } => {
                clog(&format!("[CREAM] SendInboxMessage to {}: {} chars", recipient_name, body.len()));

                let cost = toll_rates.read().inbox_message_curd;

                // Check balance from on-network user contract
                let current_balance = shared.read().user_contract
                    .as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
                if current_balance < cost {
                    clog("[CREAM] ERROR: Insufficient balance for inbox message toll");
                    return;
                }

                // Debit toll via double-entry transfer (user → root)
                let sender_name = user_state.read().moniker.clone().unwrap_or_default();
                wallet.transfer_to_root(
                    api,
                    cost,
                    "Inbox message toll".to_string(),
                    sender_name.clone(),
                ).await;

                // Look up the recipient's inbox contract key and owner.
                // First try the directory; if not found, fall back to computing from pubkey hex.
                let (recipient_inbox_key, inbox_owner) = {
                    let state = shared.read();
                    let entry = state.directory.entries.values()
                        .find(|e| e.name == recipient_name);
                    match entry {
                        Some(e) if e.inbox_contract_key.is_some() => {
                            (e.inbox_contract_key.clone().unwrap(), e.supplier.clone())
                        }
                        _ => {
                            drop(state);
                            // Fall back: compute inbox key from pubkey hex (for root/admin recipients)
                            if let Some(ref hex) = recipient_pubkey_hex {
                                let bytes = (0..hex.len())
                                    .step_by(2)
                                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0))
                                    .collect::<Vec<u8>>();
                                if bytes.len() == 32 {
                                    let mut arr = [0u8; 32];
                                    arr.copy_from_slice(&bytes);
                                    let vk = ed25519_dalek::VerifyingKey::from_bytes(&arr).unwrap();
                                    let owner = cream_common::identity::UserId(vk);
                                    let inbox_params = cream_common::inbox::InboxParameters { owner: vk };
                                    let params_bytes = serde_json::to_vec(&inbox_params).unwrap();
                                    let inbox_container = make_contract(INBOX_CONTRACT_WASM, Parameters::from(params_bytes));
                                    (inbox_container.key(), owner)
                                } else {
                                    clog(&format!("[CREAM] ERROR: Invalid pubkey hex for {}", recipient_name));
                                    return;
                                }
                            } else {
                                clog(&format!("[CREAM] ERROR: Recipient {} not found in directory or has no inbox", recipient_name));
                                return;
                            }
                        }
                    }
                };

                // GET the recipient's inbox to cache the contract locally.
                // The response will be processed by the main polling loop; we just
                // need to give Freenet a moment to fetch and cache the contract
                // before we send the UPDATE.
                let get_req = ClientRequest::ContractOp(ContractRequest::Get {
                    key: *recipient_inbox_key.id(),
                    return_contract_code: true,
                    subscribe: false,
                    blocking_subscribe: false,
                });
                if let Err(e) = api.send(get_req).await {
                    clog(&format!("[CREAM] WARNING: Failed to GET recipient inbox: {:?}", e));
                }
                gloo_timers::future::TimeoutFuture::new(2_000).await;

                let now = chrono::Utc::now();
                let msg_id: u64 = (now.timestamp_millis() as u64)
                    .wrapping_mul(1000)
                    .wrapping_add(rand_u32() as u64);

                let sender_key = user_state.read().user_contract_key.clone();

                let message = cream_common::inbox::InboxMessage {
                    id: msg_id,
                    kind,
                    from_name: sender_name,
                    from_key: sender_key,
                    body,
                    toll_paid: cost,
                    created_at: now,
                    extra: Default::default(),
                };

                // Build an update state with just this new message
                let update_state = cream_common::inbox::InboxState {
                    owner: inbox_owner,
                    messages: std::iter::once((msg_id, message.clone())).collect(),
                    updated_at: now,
                    extra: Default::default(),
                };

                let update_bytes = serde_json::to_vec(&update_state).unwrap();
                let update = ClientRequest::ContractOp(ContractRequest::Update {
                    key: recipient_inbox_key,
                    data: UpdateData::State(State::from(update_bytes)),
                });

                // Try sending the update, with one retry after a delay
                let mut sent_ok = false;
                for attempt in 0..2 {
                    if attempt > 0 {
                        clog("[CREAM] SendInboxMessage: retrying after delay...");
                        gloo_timers_sleep(2000).await;
                    }
                    match api.send(update.clone()).await {
                        Ok(_) => {
                            clog("[CREAM] SendInboxMessage: sent successfully");
                            sent_ok = true;
                            break;
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to send inbox message (attempt {}): {:?}", attempt + 1, e));
                        }
                    }
                }
                if sent_ok {
                    // Track sent message locally for display
                    shared.write().sent_messages.push(
                        crate::components::shared_state::SentMessage {
                            to_name: recipient_name.clone(),
                            message,
                        },
                    );
                }
            }

            NodeAction::SessionToll => {
                let cost = toll_rates.read().session_toll_curd;
                clog(&format!("[CREAM] SessionToll: charging {} CURD", cost));

                let current_balance = shared.read().user_contract
                    .as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
                if current_balance < cost {
                    clog("[CREAM] SessionToll: insufficient balance, skipping");
                    return;
                }

                let user_name = user_state.read().moniker.clone().unwrap_or_default();
                wallet.transfer_to_root(
                    api,
                    cost,
                    "Chat session toll".to_string(),
                    user_name,
                ).await;
                clog(&format!("[CREAM] SessionToll: paid {} CURD", cost));
            }

            NodeAction::PeerTransfer { peer_pubkey_hex, amount, description } => {
                clog(&format!("[CREAM] PeerTransfer: {} CURD to {}", amount, &peer_pubkey_hex[..16]));

                let current_balance = shared.read().user_contract
                    .as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
                if current_balance < amount {
                    clog("[CREAM] PeerTransfer: insufficient balance, skipping");
                    return;
                }

                // Derive the peer's contract key from their public key
                let pubkey_bytes: Vec<u8> = (0..peer_pubkey_hex.len())
                    .step_by(2)
                    .filter_map(|i| u8::from_str_radix(&peer_pubkey_hex[i..i+2], 16).ok())
                    .collect();
                if pubkey_bytes.len() != 32 {
                    clog("[CREAM] PeerTransfer: invalid peer pubkey hex length");
                    return;
                }
                let mut key_bytes = [0u8; 32];
                key_bytes.copy_from_slice(&pubkey_bytes);
                let peer_vk = match ed25519_dalek::VerifyingKey::from_bytes(&key_bytes) {
                    Ok(vk) => vk,
                    Err(e) => {
                        clog(&format!("[CREAM] PeerTransfer: invalid peer pubkey: {:?}", e));
                        return;
                    }
                };
                let peer_params = UserContractParameters { owner: peer_vk };
                let peer_params_bytes = serde_json::to_vec(&peer_params).unwrap();
                let peer_contract = make_contract(USER_CONTRACT_WASM, Parameters::from(peer_params_bytes));
                let peer_key = peer_contract.key();

                let user_name = user_state.read().moniker.clone().unwrap_or_default();
                wallet.do_transfer(
                    api,
                    ContractRole::User,
                    ContractRole::ThirdParty(peer_key),
                    amount,
                    description,
                    user_name,
                    "peer".to_string(),
                ).await;
                clog(&format!("[CREAM] PeerTransfer: paid {} CURD", amount));
            }

            NodeAction::SubscribeCustomerStorefront { storefront_key } => {
                clog(&format!("[CREAM] Customer mode: subscribing to storefront key '{}'", storefront_key));
                match ContractInstanceId::from_bytes(&storefront_key) {
                    Ok(sf_instance_id) => {
                        // GET the storefront state
                        let get_sf = ClientRequest::ContractOp(ContractRequest::Get {
                            key: sf_instance_id,
                            return_contract_code: false,
                            subscribe: false,
                            blocking_subscribe: false,
                        });
                        if let Err(e) = api.send(get_sf).await {
                            clog(&format!("[CREAM] ERROR: Failed to GET storefront: {:?}", e));
                        }
                        // Subscribe for live updates
                        let sub_sf = ClientRequest::ContractOp(ContractRequest::Subscribe {
                            key: sf_instance_id,
                            summary: None,
                        });
                        if let Err(e) = api.send(sub_sf).await {
                            clog(&format!("[CREAM] ERROR: Failed to subscribe to storefront: {:?}", e));
                        }
                    }
                    Err(e) => {
                        clog(&format!("[CREAM] ERROR: Invalid storefront key '{}': {:?}", storefront_key, e));
                    }
                }
            }

            NodeAction::RegisterMarket {
                name,
                description,
                venue_address,
                postcode,
                locality,
                timezone,
            } => {
                clog(&format!("[CREAM] RegisterMarket: '{}'", name));
                let organizer = key_manager.user_id();
                let location = cream_common::postcode::lookup_postcode(&postcode)
                    .unwrap_or(cream_common::location::GeoLocation::new(0.0, 0.0));

                let entry = cream_common::market::MarketEntry {
                    organizer: organizer.clone(),
                    name,
                    description,
                    venue_address,
                    location,
                    postcode: Some(postcode),
                    locality,
                    events: Vec::new(),
                    timezone,
                    suppliers: std::collections::BTreeMap::new(),
                    updated_at: chrono::Utc::now(),
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    extra: Default::default(),
                };
                let msg = entry.signable_bytes();
                let signed_entry = cream_common::market::MarketEntry {
                    signature: ed25519_dalek::Signature::from_bytes(&key_manager.sign_raw(&msg)),
                    ..entry
                };

                let mut entries = std::collections::BTreeMap::new();
                entries.insert(organizer, signed_entry.clone());
                let delta = cream_common::market::MarketDirectoryState { entries, extra: Default::default() };
                let delta_bytes = serde_json::to_vec(&delta).unwrap();

                let update = ClientRequest::ContractOp(ContractRequest::Update {
                    key: *market_directory_key,
                    data: UpdateData::Delta(StateDelta::from(delta_bytes)),
                });
                if let Err(e) = api.send(update).await {
                    clog(&format!("[CREAM] ERROR: Failed to update market directory: {:?}", e));
                } else {
                    let organizer_clone = key_manager.user_id();
                    shared.write().market_directory.entries.insert(organizer_clone, signed_entry);
                    clog("[CREAM] RegisterMarket: sent to network");
                }
            }

            NodeAction::InviteMarketSupplier { supplier_name } => {
                clog(&format!("[CREAM] InviteMarketSupplier: '{}'", supplier_name));
                let organizer = key_manager.user_id();

                let existing = shared.read().market_directory.entries.get(&organizer).cloned();
                if let Some(mut entry) = existing {
                    entry.suppliers.insert(
                        supplier_name.clone(),
                        cream_common::market::SupplierStatus::Invited,
                    );
                    entry.updated_at = chrono::Utc::now();

                    let msg = entry.signable_bytes();
                    entry.signature = ed25519_dalek::Signature::from_bytes(&key_manager.sign_raw(&msg));

                    let mut entries = std::collections::BTreeMap::new();
                    entries.insert(organizer.clone(), entry.clone());
                    let delta = cream_common::market::MarketDirectoryState { entries, extra: Default::default() };
                    let delta_bytes = serde_json::to_vec(&delta).unwrap();

                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key: *market_directory_key,
                        data: UpdateData::Delta(StateDelta::from(delta_bytes)),
                    });
                    if let Err(e) = api.send(update).await {
                        clog(&format!("[CREAM] ERROR: Failed to invite market supplier: {:?}", e));
                    } else {
                        shared.write().market_directory.entries.insert(organizer, entry);
                        clog("[CREAM] InviteMarketSupplier: updated directory (UI sends inbox separately)");
                    }
                } else {
                    clog("[CREAM] InviteMarketSupplier: no existing market entry");
                }
            }

            NodeAction::AcceptMarketInvite { market_name } => {
                // This is a no-op at the directory level — the organizer confirms acceptance.
                // The UI sends a SendInboxMessage(MarketAccept) separately.
                clog(&format!("[CREAM] AcceptMarketInvite: '{}' (inbox sent by UI)", market_name));
            }

            NodeAction::ConfirmMarketAcceptance { supplier_name } => {
                clog(&format!("[CREAM] ConfirmMarketAcceptance: '{}'", supplier_name));
                let organizer = key_manager.user_id();

                let existing = shared.read().market_directory.entries.get(&organizer).cloned();
                if let Some(mut entry) = existing {
                    if let Some(status) = entry.suppliers.get_mut(&supplier_name) {
                        if *status == cream_common::market::SupplierStatus::Invited {
                            *status = cream_common::market::SupplierStatus::Accepted;
                            entry.updated_at = chrono::Utc::now();

                            let msg = entry.signable_bytes();
                            entry.signature = ed25519_dalek::Signature::from_bytes(&key_manager.sign_raw(&msg));

                            let mut entries = std::collections::BTreeMap::new();
                            entries.insert(organizer.clone(), entry.clone());
                            let delta = cream_common::market::MarketDirectoryState { entries, extra: Default::default() };
                            let delta_bytes = serde_json::to_vec(&delta).unwrap();

                            let update = ClientRequest::ContractOp(ContractRequest::Update {
                                key: *market_directory_key,
                                data: UpdateData::Delta(StateDelta::from(delta_bytes)),
                            });
                            if let Err(e) = api.send(update).await {
                                clog(&format!("[CREAM] ERROR: Failed to confirm acceptance: {:?}", e));
                            } else {
                                shared.write().market_directory.entries.insert(organizer, entry);
                                clog("[CREAM] ConfirmMarketAcceptance: sent to network");
                            }
                        }
                    }
                }
            }

            NodeAction::UpdateMarketEvents { events } => {
                clog(&format!("[CREAM] UpdateMarketEvents: {} events", events.len()));
                let organizer = key_manager.user_id();

                let existing = shared.read().market_directory.entries.get(&organizer).cloned();
                if let Some(mut entry) = existing {
                    entry.events = events;
                    entry.updated_at = chrono::Utc::now();

                    let msg = entry.signable_bytes();
                    entry.signature = ed25519_dalek::Signature::from_bytes(&key_manager.sign_raw(&msg));

                    let mut entries = std::collections::BTreeMap::new();
                    entries.insert(organizer.clone(), entry.clone());
                    let delta = cream_common::market::MarketDirectoryState { entries, extra: Default::default() };
                    let delta_bytes = serde_json::to_vec(&delta).unwrap();

                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key: *market_directory_key,
                        data: UpdateData::Delta(StateDelta::from(delta_bytes)),
                    });
                    if let Err(e) = api.send(update).await {
                        clog(&format!("[CREAM] ERROR: Failed to update market events: {:?}", e));
                    } else {
                        shared.write().market_directory.entries.insert(organizer, entry);
                        clog("[CREAM] UpdateMarketEvents: sent to network");
                    }
                }
            }

            NodeAction::UpdateMarketDetails {
                name,
                description,
                venue_address,
                postcode,
                locality,
                timezone,
            } => {
                clog(&format!("[CREAM] UpdateMarketDetails: '{}'", name));
                let organizer = key_manager.user_id();

                let existing = shared.read().market_directory.entries.get(&organizer).cloned();
                if let Some(mut entry) = existing {
                    entry.name = name;
                    entry.description = description;
                    entry.venue_address = venue_address;
                    entry.location = cream_common::postcode::lookup_postcode(&postcode)
                        .unwrap_or(cream_common::location::GeoLocation::new(0.0, 0.0));
                    entry.postcode = Some(postcode);
                    entry.locality = locality;
                    entry.timezone = timezone;
                    entry.updated_at = chrono::Utc::now();

                    let msg = entry.signable_bytes();
                    entry.signature = ed25519_dalek::Signature::from_bytes(&key_manager.sign_raw(&msg));

                    let mut entries = std::collections::BTreeMap::new();
                    entries.insert(organizer.clone(), entry.clone());
                    let delta = cream_common::market::MarketDirectoryState { entries, extra: Default::default() };
                    let delta_bytes = serde_json::to_vec(&delta).unwrap();

                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key: *market_directory_key,
                        data: UpdateData::Delta(StateDelta::from(delta_bytes)),
                    });
                    if let Err(e) = api.send(update).await {
                        clog(&format!("[CREAM] ERROR: Failed to update market details: {:?}", e));
                    } else {
                        shared.write().market_directory.entries.insert(organizer, entry);
                        clog("[CREAM] UpdateMarketDetails: sent to network");
                    }
                }
            }

            NodeAction::UpdateMarketProducts { market_name, product_ids } => {
                clog(&format!("[CREAM] UpdateMarketProducts: '{}' with {} products", market_name, product_ids.len()));
                let my_supplier_id = key_manager.user_id();
                let moniker = user_state.read().moniker.clone().unwrap_or_default();

                let sf_key = {
                    let state = shared.read();
                    state.directory.entries.get(&my_supplier_id)
                        .map(|entry| entry.storefront_key)
                        .or_else(|| sf_contract_keys.get(&moniker).copied())
                };

                let existing = shared.read().storefronts.get(&moniker).cloned();
                if let (Some(mut sf), Some(key)) = (existing, sf_key) {
                    let product_id_set: std::collections::BTreeSet<cream_common::product::ProductId> =
                        product_ids.into_iter().map(cream_common::product::ProductId).collect();
                    sf.info.market_products.insert(market_name, product_id_set);

                    let sf_bytes = serde_json::to_vec(&sf).unwrap();
                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key,
                        data: UpdateData::State(State::from(sf_bytes)),
                    });
                    if let Err(e) = api.send(update).await {
                        clog(&format!("[CREAM] ERROR: Failed to update market products: {:?}", e));
                    } else {
                        shared.write().storefronts.insert(moniker, sf);
                        clog("[CREAM] UpdateMarketProducts: sent to network");
                    }
                }
            }

            NodeAction::CheckpointLedger => {
                clog("[CREAM] CheckpointLedger: starting checkpoint");
                let existing = shared.read().user_contract.clone();
                if let Some(mut uc_state) = existing {
                    let ledger_len = uc_state.ledger.len();
                    if ledger_len == 0 {
                        clog("[CREAM] CheckpointLedger: no transactions to checkpoint");
                        return;
                    }
                    let pruned = uc_state.checkpoint(cream_common::user_contract::PRUNE_KEEP_RECENT, chrono::Utc::now());
                    uc_state.updated_at = chrono::Utc::now();
                    uc_state.balance_curds = uc_state.derive_balance();

                    // Sign the updated state
                    let msg = uc_state.signable_bytes();
                    let key_manager: Signal<Option<crate::components::key_manager::KeyManager>> = use_context();
                    if let Some(ref km) = *key_manager.read() {
                        uc_state.signature = km.sign_user_contract(&msg);
                    }

                    let uc_bytes = serde_json::to_vec(&uc_state).unwrap();
                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key: user_contract_key_ref.unwrap(),
                        data: UpdateData::State(State::from(uc_bytes)),
                    });
                    shared.write().user_contract = Some(uc_state);

                    if let Err(e) = api.send(update).await {
                        clog(&format!("[CREAM] ERROR: CheckpointLedger update failed: {:?}", e));
                    } else {
                        clog(&format!("[CREAM] CheckpointLedger: pruned {} entries, {} remain",
                            pruned, cream_common::user_contract::PRUNE_KEEP_RECENT.min(ledger_len)));
                    }
                } else {
                    clog("[CREAM] CheckpointLedger: no user contract available");
                }
            }

            NodeAction::SetTollRates { rates } => {
                clog(&format!("[CREAM] SetTollRates: {:?}", rates));

                let existing = shared.read().root_user_contract.clone();
                if let Some(mut root_state) = existing {
                    root_state.toll_rates = rates;
                    root_state.updated_at = chrono::Utc::now();
                    root_state.balance_curds = root_state.derive_balance();

                    // Sign via FROST (root contract requires real signature)
                    let msg = root_state.signable_bytes();
                    match signing_service.sign(&msg).await {
                        Ok(sig) => root_state.signature = sig,
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: FROST signing failed for SetTollRates: {}", e));
                            return;
                        }
                    }

                    let uc_bytes = serde_json::to_vec(&root_state).unwrap();
                    let update = ClientRequest::ContractOp(ContractRequest::Update {
                        key: *root_contract_key,
                        data: UpdateData::State(State::from(uc_bytes)),
                    });
                    shared.write().root_user_contract = Some(root_state);

                    if let Err(e) = api.send(update).await {
                        clog(&format!("[CREAM] ERROR: Failed to update root contract with toll rates: {:?}", e));
                    } else {
                        clog("[CREAM] SetTollRates: root contract updated successfully");
                    }
                } else {
                    clog("[CREAM] SetTollRates: no root user contract available");
                }
            }
        }
    }

    /// Handle contract responses from the node.
    /// Returns follow-up requests to send back (e.g. PUT after NotFound,
    /// or GET+subscribe for newly discovered storefronts).
    fn handle_contract_response(
        shared: &mut Signal<crate::components::shared_state::SharedState>,
        response: ContractResponse,
        directory_instance_id: ContractInstanceId,
        subscribed: &mut HashSet<ContractInstanceId>,
        instance_to_name: &mut std::collections::HashMap<ContractInstanceId, String>,
        sf_contract_keys: &mut BTreeMap<String, ContractKey>,
        customer_supplier_name: Option<&str>,
        user_contract_instance_id: Option<ContractInstanceId>,
        root_contract_instance_id: Option<ContractInstanceId>,
        inbox_contract_instance_id: Option<ContractInstanceId>,
        market_directory_instance_id: ContractInstanceId,
    ) -> Vec<ClientRequest<'static>> {
        match response {
            ContractResponse::GetResponse { key, state, .. } => {
                let bytes = state.as_ref();
                if bytes.is_empty() {
                    return vec![];
                }
                let is_directory = *key.id() == directory_instance_id;
                let is_user_contract = user_contract_instance_id
                    .map(|id| *key.id() == id)
                    .unwrap_or(false);
                let is_root_contract = root_contract_instance_id
                    .map(|id| *key.id() == id)
                    .unwrap_or(false);
                let is_inbox = inbox_contract_instance_id
                    .map(|id| *key.id() == id)
                    .unwrap_or(false);
                let is_market_directory = *key.id() == market_directory_instance_id;
                clog(&format!("[CREAM] GetResponse: is_directory={}, is_user_contract={}, is_root={}, is_inbox={}, is_market_dir={}, bytes_len={}",
                    is_directory, is_user_contract, is_root_contract, is_inbox, is_market_directory, bytes.len()));
                if is_market_directory {
                    match serde_json::from_slice::<cream_common::market::MarketDirectoryState>(bytes) {
                        Ok(mkt_state) => {
                            clog(&format!("[CREAM] Market directory GET: {} markets",
                                mkt_state.entries.len()));
                            shared.write().market_directory = mkt_state;
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse market directory GetResponse: {e}"));
                        }
                    }
                } else if is_inbox {
                    match serde_json::from_slice::<cream_common::inbox::InboxState>(bytes) {
                        Ok(inbox_state) => {
                            clog(&format!("[CREAM] Inbox contract GET: {} messages",
                                inbox_state.messages.len()));
                            shared.write().inbox = Some(inbox_state);
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse inbox GetResponse: {e}"));
                        }
                    }
                } else if is_root_contract {
                    match serde_json::from_slice::<UserContractState>(bytes) {
                        Ok(uc_state) => {
                            clog(&format!("[CREAM] Root contract GET: balance={}, ledger_len={}",
                                uc_state.balance_curds, uc_state.ledger.len()));
                            shared.write().root_user_contract = Some(uc_state);
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse root contract GetResponse: {e}"));
                        }
                    }
                } else if is_user_contract {
                    match serde_json::from_slice::<UserContractState>(bytes) {
                        Ok(uc_state) => {
                            clog(&format!("[CREAM] User contract GET: name='{}', balance={}",
                                uc_state.name, uc_state.balance_curds));
                            shared.write().user_contract = Some(uc_state);
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse user contract GetResponse: {e}"));
                        }
                    }
                } else if is_directory {
                    match serde_json::from_slice::<DirectoryState>(bytes) {
                        Ok(directory) => {
                            clog(&format!("[CREAM] Directory GET: {} entries: {:?}",
                                directory.entries.len(),
                                directory.entries.values().map(|e| e.name.as_str()).collect::<Vec<_>>()
                            ));
                            let follow_ups =
                                subscribe_new_storefronts(&directory, subscribed, instance_to_name);
                            clog(&format!("[CREAM] Sending {} follow-up requests", follow_ups.len()));
                            shared.write().directory = directory;
                            return follow_ups;
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse directory GetResponse: {e}"));
                        }
                    }
                } else {
                    match serde_json::from_slice::<StorefrontState>(bytes) {
                        Ok(storefront) => {
                            // Look up the directory entry by the storefront's owner (UserId).
                            // This maps e.g. info.name "Gary's Farm" → directory name "Gary".
                            let dir_name = {
                                let state = shared.read();
                                state.directory.entries.get(&storefront.info.owner)
                                    .map(|e| e.name.clone())
                            };
                            let name_from_map = instance_to_name.get(key.id()).cloned();
                            // Prefer directory name (correct case) over rendezvous name
                            // (lowercase). Route parameters are resolved case-insensitively
                            // so "gary" from rendezvous will still match "Gary" from directory.
                            let name = dir_name
                                .or(customer_supplier_name.map(|s| s.to_string()))
                                .or(name_from_map)
                                .unwrap_or_else(|| storefront.info.name.clone());
                            clog(&format!("[CREAM] Storefront GET: keyed as '{}' (info.name='{}', owner={:?}, {} products)",
                                name, storefront.info.name, storefront.info.owner, storefront.products.len()));
                            // Store the ContractKey for later use (e.g. PlaceOrder)
                            sf_contract_keys.insert(name.clone(), key);
                            shared.write().storefronts.insert(name, storefront);
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse storefront GetResponse: {e}"));
                        }
                    }
                }
            }

            ContractResponse::UpdateNotification { key, update, .. } => {
                let bytes = match &update {
                    UpdateData::State(s) => s.as_ref(),
                    UpdateData::Delta(d) => d.as_ref(),
                    UpdateData::StateAndDelta { state, .. } => state.as_ref(),
                    _ => return vec![],
                };
                if bytes.is_empty() {
                    return vec![];
                }
                let is_directory = *key.id() == directory_instance_id;
                let is_user_contract = user_contract_instance_id
                    .map(|id| *key.id() == id)
                    .unwrap_or(false);
                let is_root_contract = root_contract_instance_id
                    .map(|id| *key.id() == id)
                    .unwrap_or(false);
                let is_inbox = inbox_contract_instance_id
                    .map(|id| *key.id() == id)
                    .unwrap_or(false);
                let is_market_directory = *key.id() == market_directory_instance_id;
                clog(&format!("[CREAM] UpdateNotification: is_directory={}, is_user_contract={}, is_root={}, is_inbox={}, is_market_dir={}, bytes_len={}",
                    is_directory, is_user_contract, is_root_contract, is_inbox, is_market_directory, bytes.len()));
                if is_market_directory {
                    match serde_json::from_slice::<cream_common::market::MarketDirectoryState>(bytes) {
                        Ok(mkt_update) => {
                            clog(&format!("[CREAM] Market directory notification: {} markets",
                                mkt_update.entries.len()));
                            shared.write().market_directory.merge(mkt_update);
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse market directory notification: {e}"));
                        }
                    }
                } else if is_inbox {
                    match serde_json::from_slice::<cream_common::inbox::InboxState>(bytes) {
                        Ok(inbox_update) => {
                            clog(&format!("[CREAM] Inbox notification: {} messages",
                                inbox_update.messages.len()));
                            let mut state = shared.write();
                            if let Some(existing) = state.inbox.as_mut() {
                                existing.merge(inbox_update);
                            } else {
                                state.inbox = Some(inbox_update);
                            }
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse inbox notification: {e}"));
                        }
                    }
                } else if is_root_contract {
                    match serde_json::from_slice::<UserContractState>(bytes) {
                        Ok(uc_update) => {
                            clog(&format!("[CREAM] Root contract notification: balance={}, ledger_len={}",
                                uc_update.balance_curds, uc_update.ledger.len()));
                            let mut state = shared.write();
                            if let Some(existing) = state.root_user_contract.as_mut() {
                                existing.merge(uc_update);
                            } else {
                                state.root_user_contract = Some(uc_update);
                            }
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse root contract notification: {e}"));
                        }
                    }
                } else if is_user_contract {
                    match serde_json::from_slice::<UserContractState>(bytes) {
                        Ok(uc_update) => {
                            clog(&format!("[CREAM] User contract notification: name='{}', balance={}",
                                uc_update.name, uc_update.balance_curds));
                            let mut state = shared.write();
                            if let Some(existing) = state.user_contract.as_mut() {
                                existing.merge(uc_update);
                            } else {
                                state.user_contract = Some(uc_update);
                            }
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse user contract notification: {e}"));
                        }
                    }
                } else if is_directory {
                    match serde_json::from_slice::<DirectoryState>(bytes) {
                        Ok(dir_update) => {
                            clog(&format!("[CREAM] Directory notification: {} entries", dir_update.entries.len()));
                            let follow_ups =
                                subscribe_new_storefronts(&dir_update, subscribed, instance_to_name);
                            shared.write().directory.merge(dir_update);
                            return follow_ups;
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse directory notification: {e}"));
                        }
                    }
                } else {
                    match serde_json::from_slice::<StorefrontState>(bytes) {
                        Ok(sf_update) => {
                            let dir_name = {
                                let state = shared.read();
                                state.directory.entries.get(&sf_update.info.owner)
                                    .map(|e| e.name.clone())
                            };
                            let name_from_map = instance_to_name.get(key.id()).cloned();
                            let name = customer_supplier_name.map(|s| s.to_string())
                                .or(dir_name)
                                .or(name_from_map)
                                .unwrap_or_else(|| sf_update.info.name.clone());
                            clog(&format!("[CREAM] Storefront notification: keyed as '{}' ({} products)",
                                name, sf_update.products.len()));
                            let mut state = shared.write();
                            if let Some(existing) = state.storefronts.get_mut(&name) {
                                existing.merge(sf_update);
                            } else {
                                state.storefronts.insert(name, sf_update);
                            }
                        }
                        Err(e) => {
                            clog(&format!("[CREAM] ERROR: Failed to parse storefront notification: {e}"));
                        }
                    }
                }
            }

            ContractResponse::PutResponse { key } => {
                tracing::info!("Contract put OK: {:?}", key);
            }

            ContractResponse::UpdateResponse { key, .. } => {
                clog(&format!("[CREAM] UpdateResponse OK: {:?}", key));
            }

            ContractResponse::SubscribeResponse { key, subscribed } => {
                tracing::info!(
                    "Subscription {:?}: {}",
                    key,
                    if subscribed { "active" } else { "failed" }
                );
            }

            ContractResponse::NotFound { instance_id } => {
                if instance_id == directory_instance_id {
                    // Directory contract doesn't exist yet — we're the first tab.
                    // PUT it with empty state + subscribe.
                    tracing::info!("Directory not found, creating it...");
                    let directory_contract = make_contract(
                        DIRECTORY_CONTRACT_WASM,
                        Parameters::from(vec![]),
                    );
                    let empty_dir = DirectoryState::default();
                    let initial_state =
                        serde_json::to_vec(&empty_dir).unwrap();
                    return vec![ClientRequest::ContractOp(
                        ContractRequest::Put {
                            contract: directory_contract,
                            state: WrappedState::new(initial_state),
                            related_contracts: RelatedContracts::default(),
                            subscribe: true,
                            blocking_subscribe: false,
                        },
                    )];
                } else {
                    tracing::warn!(
                        "Contract not found: {:?}",
                        instance_id
                    );
                }
            }

            _ => {
                tracing::debug!("Unhandled contract response");
            }
        }
        vec![]
    }

    /// For each supplier in the directory whose storefront we haven't
    /// subscribed to yet, emit a GET request and an explicit Subscribe.
    fn subscribe_new_storefronts(
        directory: &DirectoryState,
        subscribed: &mut HashSet<ContractInstanceId>,
        instance_to_name: &mut std::collections::HashMap<ContractInstanceId, String>,
    ) -> Vec<ClientRequest<'static>> {
        let mut requests = Vec::new();
        for entry in directory.entries.values() {
            let instance_id = *entry.storefront_key.id();
            // Always update the name mapping (in case directory was updated)
            instance_to_name.insert(instance_id, entry.name.clone());
            if subscribed.insert(instance_id) {
                clog(&format!("[CREAM] Auto-subscribing to storefront for {} (instance_id={:?})", entry.name, instance_id));
                // GET the current state
                requests.push(ClientRequest::ContractOp(
                    ContractRequest::Get {
                        key: instance_id,
                        return_contract_code: false,
                        subscribe: false,
                        blocking_subscribe: false,
                    },
                ));
                // Explicitly subscribe for live updates
                requests.push(ClientRequest::ContractOp(
                    ContractRequest::Subscribe {
                        key: instance_id,
                        summary: None,
                    },
                ));
            }
        }
        requests
    }
}

#[cfg(target_family = "wasm")]
async fn node_comms(rx: UnboundedReceiver<NodeAction>) {
    wasm_impl::node_comms(rx).await;
}

// Non-WASM stub (e.g. running tests natively)
#[cfg(not(target_family = "wasm"))]
async fn node_comms(mut rx: UnboundedReceiver<NodeAction>) {
    use futures::StreamExt;
    tracing::warn!("Not running in WASM; node_comms is a no-op");
    while let Some(action) = rx.next().await {
        tracing::debug!("Node action (native stub): {:?}", action);
    }
}
