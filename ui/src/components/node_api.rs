use dioxus::prelude::*;

/// Actions the UI can send to the Freenet node via the coroutine.
#[derive(Debug, Clone)]
pub enum NodeAction {
    /// Register as a supplier in the directory contract.
    RegisterSupplier {
        name: String,
        postcode: String,
        description: String,
    },
    /// Deploy a new storefront contract for this supplier.
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
        quantity_available: u32,
    },
    /// Remove a product from the storefront.
    RemoveProduct { product_id: String },
    /// Place an order on a storefront.
    PlaceOrder {
        storefront_name: String,
        product_id: String,
        quantity: u32,
        deposit_tier: String,
    },
    /// Subscribe to a specific storefront's updates.
    SubscribeStorefront { supplier_name: String },
}

/// Get a handle to send actions to the node communication coroutine.
pub fn use_node_action() -> Coroutine<NodeAction> {
    use_coroutine_handle::<NodeAction>()
}

/// Start the node communication coroutine.
///
/// When `use-node` feature is enabled, connects via WebSocket to a Freenet node
/// and handles contract operations. Otherwise, acts as a no-op sink.
pub fn use_node_coroutine() {
    #[cfg(not(feature = "use-node"))]
    {
        use_coroutine(|mut rx: UnboundedReceiver<NodeAction>| async move {
            use futures::StreamExt;
            while let Some(action) = rx.next().await {
                tracing::debug!("Node action (offline mode): {:?}", action);
            }
        });
    }

    #[cfg(feature = "use-node")]
    {
        use_coroutine(|rx: UnboundedReceiver<NodeAction>| node_comms(rx));
    }
}

// ─── WASM + use-node implementation ─────────────────────────────────────────

#[cfg(all(target_family = "wasm", feature = "use-node"))]
mod wasm_impl {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Arc;

    use dioxus::prelude::*;
    use futures::channel::mpsc;
    use futures::{SinkExt, StreamExt};

    use cream_common::directory::{DirectoryEntry, DirectoryState};
    use cream_common::location::GeoLocation;
    #[allow(unused_imports)]
    use cream_common::order::OrderId;
    use cream_common::product::{Product, ProductCategory, ProductId};
    use cream_common::storefront::{
        SignedProduct, StorefrontInfo, StorefrontParameters, StorefrontState,
    };
    use freenet_stdlib::client_api::{
        ClientError, ClientRequest, ContractRequest, ContractResponse, HostResponse,
    };
    use freenet_stdlib::prelude::*;

    use super::NodeAction;
    use crate::components::key_manager::KeyManager;
    use crate::components::shared_state::use_shared_state;

    /// Embedded directory contract WASM (built with `cargo make build-contracts-dev`).
    const DIRECTORY_CONTRACT_WASM: &[u8] = include_bytes!(
        "../../../target/wasm32-unknown-unknown/release/cream_directory_contract.wasm"
    );

    /// Embedded storefront contract WASM (built with `cargo make build-contracts-dev`).
    const STOREFRONT_CONTRACT_WASM: &[u8] = include_bytes!(
        "../../../target/wasm32-unknown-unknown/release/cream_storefront_contract.wasm"
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

        // ── Connect to node via WebSocket ───────────────────────────────
        const DEFAULT_NODE_URL: &str =
            "ws://localhost:3001/v1/contract/command?encodingProtocol=native";
        let node_url = option_env!("CREAM_NODE_URL").unwrap_or(DEFAULT_NODE_URL);

        let conn = match web_sys::WebSocket::new(node_url) {
            Ok(c) => c,
            Err(e) => {
                shared.write().last_error =
                    Some(format!("WebSocket connection failed: {:?}", e));
                return;
            }
        };

        let (send_responses, mut host_responses) = mpsc::unbounded();
        let (_send_half, mut requests) = mpsc::unbounded::<ClientRequest<'static>>();

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

        // ── Set up directory contract ─────────────────────────────────
        let directory_contract =
            make_contract(DIRECTORY_CONTRACT_WASM, Parameters::from(vec![]));
        let directory_key = directory_contract.key();
        let directory_instance_id = *directory_key.id();

        tracing::info!("Directory contract key: {:?}", directory_key);
        shared.write().directory_contract_key =
            Some(format!("{}", directory_key));

        // Try to GET the existing directory.
        // If it doesn't exist yet, we'll PUT it when we get NotFound.
        let get_request = ClientRequest::ContractOp(ContractRequest::Get {
            key: directory_instance_id,
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
            key: directory_instance_id,
            summary: None,
        });
        tracing::info!("Subscribing to directory contract...");
        if let Err(e) = api.send(subscribe_dir).await {
            tracing::error!("Failed to subscribe to directory: {:?}", e);
        }

        // Local map of supplier name -> ContractKey for storefront updates
        let mut sf_contract_keys: BTreeMap<String, ContractKey> = BTreeMap::new();

        // Track which storefronts we've already subscribed to
        let mut subscribed_storefronts: HashSet<ContractInstanceId> = HashSet::new();

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
                    ).await;
                }

                response = host_responses.next() => {
                    let Some(response) = response else { break };
                    match response {
                        Ok(HostResponse::ContractResponse(cr)) => {
                            let follow_ups = handle_contract_response(
                                &mut shared, cr, directory_instance_id,
                                &mut subscribed_storefronts,
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

    /// Convert a UI action into contract operations and send them.
    async fn handle_action(
        action: NodeAction,
        api: &mut freenet_stdlib::client_api::WebApi,
        shared: &mut Signal<crate::components::shared_state::SharedState>,
        directory_key: &ContractKey,
        sf_contract_keys: &mut BTreeMap<String, ContractKey>,
        key_manager: &KeyManager,
    ) {
        match action {
            NodeAction::RegisterSupplier {
                name,
                postcode,
                description,
            } => {
                // Deploy a storefront contract for this supplier using real keys.
                let supplier_id = key_manager.supplier_id();
                let owner_key = key_manager.supplier_verifying_key();

                // Look up postcode to get coordinates
                let location =
                    cream_common::postcode::lookup_au_postcode(&postcode)
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
                    },
                    products: BTreeMap::new(),
                    orders: BTreeMap::new(),
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

                tracing::info!("Deploying storefront for {}: {:?}", name, sf_key);
                if let Err(e) = api.send(put_sf).await {
                    tracing::error!("Failed to deploy storefront: {:?}", e);
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

                // Now register in the directory with a real signature
                let mut entry = DirectoryEntry {
                    supplier: supplier_id,
                    name: name.clone(),
                    description,
                    location,
                    categories: vec![],
                    storefront_key: sf_key,
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

                tracing::info!("Registering {} in directory", name);
                if let Err(e) = api.send(update_dir).await {
                    tracing::error!("Failed to update directory: {:?}", e);
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
                quantity_available,
            } => {
                // Find this user's storefront key from SharedState
                let user_name = {
                    let state = shared.read();
                    // Find our storefront - look for any storefront key we deployed
                    state.storefront_keys.keys().next().cloned()
                };

                let Some(supplier_name) = user_name else {
                    tracing::warn!("No storefront deployed yet, can't add product");
                    return;
                };

                let Some(sf_key) = sf_contract_keys.get(&supplier_name).cloned() else {
                    tracing::warn!("Storefront key not found for {}", supplier_name);
                    return;
                };

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
                    quantity_available,
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

                    let update =
                        ClientRequest::ContractOp(ContractRequest::Update {
                            key: sf_key,
                            data: UpdateData::State(State::from(sf_bytes)),
                        });

                    // Update local SharedState immediately so the supplier sees their product
                    shared.write().storefronts.insert(supplier_name.clone(), sf);

                    tracing::info!("Adding product to storefront");
                    if let Err(e) = api.send(update).await {
                        tracing::error!("Failed to add product: {:?}", e);
                    }
                } else {
                    tracing::warn!(
                        "Storefront state not found for {}",
                        supplier_name
                    );
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
            } => {
                tracing::info!(
                    "PlaceOrder on {}: {} x{} ({})",
                    storefront_name,
                    product_id,
                    quantity,
                    deposit_tier
                );
                // TODO: Build Order, add to storefront state, send Update
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
    ) -> Vec<ClientRequest<'static>> {
        match response {
            ContractResponse::GetResponse { state, .. } => {
                let bytes = state.as_ref();
                if bytes.is_empty() {
                    return vec![];
                }
                if let Ok(directory) =
                    serde_json::from_slice::<DirectoryState>(bytes)
                {
                    tracing::info!(
                        "Got directory state: {} entries",
                        directory.entries.len()
                    );
                    let follow_ups =
                        subscribe_new_storefronts(&directory, subscribed);
                    shared.write().directory = directory;
                    return follow_ups;
                } else if let Ok(storefront) =
                    serde_json::from_slice::<StorefrontState>(bytes)
                {
                    tracing::info!(
                        "Got storefront: {} ({} products)",
                        storefront.info.name,
                        storefront.products.len()
                    );
                    let name = storefront.info.name.clone();
                    shared.write().storefronts.insert(name, storefront);
                }
            }

            ContractResponse::UpdateNotification { update, .. } => {
                let bytes = match &update {
                    UpdateData::State(s) => s.as_ref(),
                    UpdateData::Delta(d) => d.as_ref(),
                    _ => return vec![],
                };
                if bytes.is_empty() {
                    return vec![];
                }
                if let Ok(dir_update) =
                    serde_json::from_slice::<DirectoryState>(bytes)
                {
                    tracing::info!(
                        "Directory update: {} entries",
                        dir_update.entries.len()
                    );
                    let follow_ups =
                        subscribe_new_storefronts(&dir_update, subscribed);
                    shared.write().directory.merge(dir_update);
                    return follow_ups;
                } else if let Ok(sf_update) =
                    serde_json::from_slice::<StorefrontState>(bytes)
                {
                    let name = sf_update.info.name.clone();
                    let mut state = shared.write();
                    if let Some(existing) = state.storefronts.get_mut(&name) {
                        existing.merge(sf_update);
                    } else {
                        state.storefronts.insert(name, sf_update);
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
    ) -> Vec<ClientRequest<'static>> {
        let mut requests = Vec::new();
        for entry in directory.entries.values() {
            let instance_id = *entry.storefront_key.id();
            if subscribed.insert(instance_id) {
                tracing::info!(
                    "Auto-subscribing to storefront for {}",
                    entry.name
                );
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

#[cfg(all(target_family = "wasm", feature = "use-node"))]
async fn node_comms(rx: UnboundedReceiver<NodeAction>) {
    wasm_impl::node_comms(rx).await;
}

// Non-WASM stub for `use-node` feature (e.g. running tests natively)
#[cfg(all(not(target_family = "wasm"), feature = "use-node"))]
async fn node_comms(mut rx: UnboundedReceiver<NodeAction>) {
    use futures::StreamExt;
    tracing::warn!("use-node enabled but not running in WASM; node_comms is a no-op");
    while let Some(action) = rx.next().await {
        tracing::debug!("Node action (native stub): {:?}", action);
    }
}
