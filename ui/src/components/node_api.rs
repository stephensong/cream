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
    /// Faucet: transfer 1000 CURD from root to the current user.
    FaucetTopUp,
    /// Send a message to a supplier's storefront (costs 10 CURD toll).
    SendMessage {
        supplier_name: String,
        body: String,
        reply_to: Option<u64>,
    },
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

        // ── Subscribe to root user contract ─────────────────────────────
        // Root's identity is deterministic, so we can derive its contract key.
        let root_vk = cream_common::identity::root_customer_id().0;
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
                            tracing::debug!("Unhandled response: {:?}", other);
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
                            } else {
                                tracing::error!("Node error: {:?}", e);
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
        };

        // Resolve sender key
        let sender_key = match &sender {
            ContractRole::Root => Some(*root_contract_key),
            ContractRole::User => user_contract_key.copied(),
            ContractRole::ThirdParty(key) => Some(*key),
        };
        if let Some(key) = sender_key {
            update_contract_ledger(api, shared, &sender, key, debit).await;
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
            update_contract_ledger(api, shared, &receiver, key, credit).await;
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
    ) {
        // ThirdParty: construct a minimal state with just the transaction entry.
        // The merge logic does ledger union unconditionally, so the credit gets
        // appended without overwriting the target's metadata. No network GET needed.
        if matches!(role, ContractRole::ThirdParty(_)) {
            // Use a dummy key for the minimal state — credit-only updates
            // are accepted without signature verification
            let dummy_key = ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]);
            let minimal_state = UserContractState {
                owner: cream_common::identity::CustomerId(dummy_key.verifying_key()),
                name: String::new(),
                origin_supplier: String::new(),
                current_supplier: String::new(),
                balance_curds: 0,
                invited_by: String::new(),
                ledger: vec![tx],
                next_tx_id: 0,
                updated_at: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
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
                    cream_common::identity::root_sign(&uc.signable_bytes())
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
    ) {
        // Construct wallet backend for this action dispatch
        let mut wallet = CreamNativeWallet::new(
            *shared,
            *root_contract_key,
            *user_contract_key_ref,
        );

        match action {
            NodeAction::RegisterSupplier {
                name,
                postcode,
                locality,
                description,
            } => {
                let supplier_id = key_manager.supplier_id();
                let owner_key = key_manager.supplier_verifying_key();

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

                    // Still register with rendezvous service so customers can discover us
                    if !is_customer {
                        let rendezvous_name = name.to_lowercase().replace(' ', "-");
                        let node_address = node_url.to_string();
                        let sf_key_str = format!("{}", sf_key);
                        let pub_key_bytes = key_manager.supplier_verifying_key().as_bytes().to_vec();
                        let pub_key_hex: String = pub_key_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                        let sign_msg = format!("{}|{}|{}", rendezvous_name, node_address, sf_key_str);
                        let sig_bytes = key_manager.sign_raw(sign_msg.as_bytes());
                        let sig_hex: String = sig_bytes.iter().map(|b| format!("{:02x}", b)).collect();

                        let rname = rendezvous_name.clone();
                        let raddr = node_address.clone();
                        let reg_pub_hex = pub_key_hex.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match crate::components::rendezvous::register_supplier(
                                &rname, &raddr, &sf_key_str, &reg_pub_hex, &sig_hex,
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
                    .or_else(|| cream_common::postcode::lookup_au_postcode(&postcode))
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
                    },
                    products: BTreeMap::new(),
                    orders: BTreeMap::new(),
                    messages: BTreeMap::new(),
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
                    owner: cream_common::identity::CustomerId(owner_key),
                    name: name.clone(),
                    origin_supplier: name.clone(),
                    current_supplier: name.clone(),
                    balance_curds: 0,
                    invited_by: String::new(),
                    ledger: Vec::new(),
                    next_tx_id: 0,
                    updated_at: chrono::Utc::now(),
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
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
                    updated_at: chrono::Utc::now(),
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
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
                let dir_update = DirectoryState { entries };
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
                    let pub_key_bytes = key_manager.supplier_verifying_key().as_bytes().to_vec();
                    let pub_key_hex: String = pub_key_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                    let sign_msg = format!("{}|{}|{}", rendezvous_name, node_address, sf_key_str);
                    let sig_bytes = key_manager.sign_raw(sign_msg.as_bytes());
                    let sig_hex: String = sig_bytes.iter().map(|b| format!("{:02x}", b)).collect();

                    let rname = rendezvous_name.clone();
                    let raddr = node_address.clone();
                    let reg_pub_hex = pub_key_hex.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        match crate::components::rendezvous::register_supplier(
                            &rname, &raddr, &sf_key_str, &reg_pub_hex, &sig_hex,
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
                                let messages_changed = sf.prune_old_messages(now);
                                if orders_changed || messages_changed {
                                    if orders_changed {
                                        clog(&format!("[CREAM] Expired orders for '{}'", expiry_supplier));
                                    }
                                    if messages_changed {
                                        clog(&format!("[CREAM] Pruned old messages for '{}'", expiry_supplier));
                                    }
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
                // Find this user's storefront key from the directory using their SupplierId.
                // This works whether the storefront was deployed by this tab (RegisterSupplier)
                // or pre-populated by the test harness / another session.
                let my_supplier_id = key_manager.supplier_id();
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
                };
                let signature = key_manager.sign_product(&product);
                let signed_product = SignedProduct {
                    product,
                    signature,
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
                    customer: key_manager.customer_id(),
                    quantity,
                    deposit_tier: tier,
                    deposit_amount,
                    total_price,
                    status: OrderStatus::Reserved { expires_at },
                    created_at: now,
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    escrow_token: None,
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
                let my_supplier_id = key_manager.supplier_id();
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
                            .map(|s: &str| s.to_string())
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
                let my_supplier_id = key_manager.supplier_id();
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
                let my_supplier_id = key_manager.supplier_id();
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
                let my_supplier_id = key_manager.supplier_id();
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
                let my_supplier_id = key_manager.supplier_id();
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

                let owner_key = key_manager.customer_verifying_key();
                let uc_params = UserContractParameters { owner: owner_key };
                let params_bytes = serde_json::to_vec(&uc_params).unwrap();
                let uc_contract = make_contract(
                    USER_CONTRACT_WASM,
                    Parameters::from(params_bytes),
                );
                let uc_key = uc_contract.key();

                let now = chrono::Utc::now();
                let uc_state = UserContractState {
                    owner: key_manager.customer_id(),
                    name: name.clone(),
                    origin_supplier,
                    current_supplier,
                    balance_curds: 0,
                    invited_by,
                    ledger: Vec::new(),
                    next_tx_id: 0,
                    updated_at: now,
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
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

            NodeAction::SendMessage {
                supplier_name,
                body,
                reply_to,
            } => {
                clog(&format!("[CREAM] SendMessage to {}: {} chars", supplier_name, body.len()));

                // Check balance from on-network user contract
                let current_balance = shared.read().user_contract
                    .as_ref().map(|uc| uc.balance_curds).unwrap_or(0);
                if current_balance < 10 {
                    clog("[CREAM] ERROR: Insufficient balance for message toll (10 CURD)");
                    return;
                }

                // Debit 10 CURD toll via double-entry transfer (user → root)
                let sender_name = user_state.read().moniker.clone().unwrap_or_default();
                wallet.transfer_to_root(
                    api,
                    10,
                    "Message toll".to_string(),
                    sender_name,
                ).await;

                // Find the storefront's contract key
                let sf_key = sf_contract_keys.get(&supplier_name).copied()
                    .or_else(|| {
                        let state = shared.read();
                        state.directory.entries.values()
                            .find(|e| e.name == supplier_name)
                            .map(|e| e.storefront_key)
                    });

                let Some(sf_key) = sf_key else {
                    clog(&format!("[CREAM] ERROR: No storefront key found for {}", supplier_name));
                    return;
                };

                let existing_sf = shared.read().storefronts.get(&supplier_name).cloned();
                let Some(mut sf) = existing_sf else {
                    clog(&format!("[CREAM] ERROR: Storefront state not found for {}", supplier_name));
                    return;
                };

                let now = chrono::Utc::now();
                let msg_id: u64 = (now.timestamp_millis() as u64)
                    .wrapping_mul(1000)
                    .wrapping_add(rand_u32() as u64);

                let sender_name = user_state.read().moniker.clone().unwrap_or_default();
                let sender_key = user_state.read().user_contract_key.clone();

                let message = cream_common::message::Message {
                    id: msg_id,
                    sender_name,
                    sender_key,
                    body,
                    toll_paid: 10,
                    created_at: now,
                    reply_to,
                };

                sf.messages.insert(msg_id, message);

                let sf_bytes = serde_json::to_vec(&sf).unwrap();
                let update = ClientRequest::ContractOp(ContractRequest::Update {
                    key: sf_key,
                    data: UpdateData::State(State::from(sf_bytes)),
                });

                shared.write().storefronts.insert(supplier_name.clone(), sf);

                if let Err(e) = api.send(update).await {
                    clog(&format!("[CREAM] ERROR: Failed to send message: {:?}", e));
                } else {
                    clog("[CREAM] SendMessage: sent successfully");
                }
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
                clog(&format!("[CREAM] GetResponse: is_directory={}, is_user_contract={}, is_root={}, bytes_len={}",
                    is_directory, is_user_contract, is_root_contract, bytes.len()));
                if is_root_contract {
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
                            // Look up the directory entry by the storefront's owner (SupplierId).
                            // This maps e.g. info.name "Gary's Farm" → directory name "Gary".
                            let dir_name = {
                                let state = shared.read();
                                state.directory.entries.get(&storefront.info.owner)
                                    .map(|e| e.name.clone())
                            };
                            let name_from_map = instance_to_name.get(key.id()).cloned();
                            // In customer mode, use the connected_supplier name so StorefrontView
                            // can look it up by the route parameter (which comes from rendezvous).
                            let name = customer_supplier_name.map(|s| s.to_string())
                                .or(dir_name)
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
                clog(&format!("[CREAM] UpdateNotification: is_directory={}, is_user_contract={}, is_root={}, bytes_len={}",
                    is_directory, is_user_contract, is_root_contract, bytes.len()));
                if is_root_contract {
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
                tracing::debug!("Contract update OK: {:?}", key);
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
