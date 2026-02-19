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
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use dioxus::prelude::*;
    use futures::channel::mpsc;
    use futures::{SinkExt, StreamExt};

    use cream_common::directory::{DirectoryEntry, DirectoryState};
    use cream_common::identity::SupplierId;
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

        // ── Connect to node via WebSocket ───────────────────────────────
        let conn = match web_sys::WebSocket::new(
            "ws://localhost:3001/v1/contract/command?encodingProtocol=native",
        ) {
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

        // ── Deploy directory contract ───────────────────────────────────
        let directory_contract =
            make_contract(DIRECTORY_CONTRACT_WASM, Parameters::from(vec![]));
        let directory_key = directory_contract.key();
        let _directory_instance_id = *directory_key.id();

        tracing::info!("Directory contract key: {:?}", directory_key);
        shared.write().directory_contract_key =
            Some(format!("{}", directory_key));

        // PUT the directory contract with empty initial state, and subscribe
        let empty_dir = DirectoryState::default();
        let initial_state = serde_json::to_vec(&empty_dir).unwrap();
        let put_request = ClientRequest::ContractOp(ContractRequest::Put {
            contract: directory_contract.clone(),
            state: WrappedState::new(initial_state),
            related_contracts: RelatedContracts::default(),
            subscribe: true,
            blocking_subscribe: false,
        });

        tracing::info!("Putting directory contract...");
        if let Err(e) = api.send(put_request).await {
            tracing::error!("Failed to PUT directory contract: {:?}", e);
            shared.write().last_error =
                Some(format!("Failed to deploy directory contract: {:?}", e));
        }

        // ── Main event loop ─────────────────────────────────────────────
        loop {
            futures::select! {
                action = rx.next() => {
                    let Some(action) = action else { break };
                    handle_action(
                        action,
                        &mut api,
                        &mut shared,
                        &directory_key,
                    ).await;
                }

                response = host_responses.next() => {
                    let Some(response) = response else { break };
                    match response {
                        Ok(HostResponse::ContractResponse(cr)) => {
                            handle_contract_response(&mut shared, cr);
                        }
                        Ok(HostResponse::Ok) => {
                            tracing::debug!("Node OK");
                        }
                        Ok(other) => {
                            tracing::debug!("Unhandled response: {:?}", other);
                        }
                        Err(e) => {
                            tracing::error!("Node error: {:?}", e);
                            shared.write().last_error = Some(format!("{:?}", e));
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
    ) {
        match action {
            NodeAction::RegisterSupplier {
                name,
                postcode,
                description,
            } => {
                // First, deploy a storefront contract for this supplier.
                // Use a dummy verifying key for dev mode (signature checks are bypassed).
                let dummy_key = ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]);
                let dummy_key = match dummy_key {
                    Ok(k) => k,
                    Err(_) => {
                        // Use a valid point on the curve
                        let signing_key =
                            ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]);
                        ed25519_dalek::VerifyingKey::from(&signing_key)
                    }
                };

                let supplier_id = SupplierId(dummy_key);

                // Look up postcode to get coordinates
                let location =
                    cream_common::postcode::lookup_au_postcode(&postcode)
                        .unwrap_or(GeoLocation::new(-33.87, 151.21)); // Default to Sydney

                let storefront_params = StorefrontParameters { owner: dummy_key };
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

                // Store the storefront key
                shared
                    .write()
                    .storefront_keys
                    .insert(name.clone(), format!("{}", sf_key));

                // Now register in the directory
                let mut entries = BTreeMap::new();
                let entry = DirectoryEntry {
                    supplier: supplier_id,
                    name: name.clone(),
                    description,
                    location,
                    categories: vec![],
                    storefront_key: sf_key,
                    updated_at: chrono::Utc::now(),
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                };
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

                let sf_key_str = {
                    let state = shared.read();
                    state.storefront_keys.get(&supplier_name).cloned()
                };

                let Some(_sf_key_str) = sf_key_str else {
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
                let signed_product = SignedProduct {
                    product,
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
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
                    sf.products.insert(product_id, signed_product);
                    let sf_bytes = serde_json::to_vec(&sf).unwrap();

                    // Find the actual ContractKey to send the update
                    // We need to reconstruct it from the storefront params
                    let dummy_key =
                        ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]);
                    let verifying = ed25519_dalek::VerifyingKey::from(&dummy_key);
                    let params = StorefrontParameters { owner: verifying };
                    let params_bytes = serde_json::to_vec(&params).unwrap();
                    let sf_contract = make_contract(
                        STOREFRONT_CONTRACT_WASM,
                        Parameters::from(params_bytes),
                    );

                    let update =
                        ClientRequest::ContractOp(ContractRequest::Update {
                            key: sf_contract.key(),
                            data: UpdateData::State(State::from(sf_bytes)),
                        });

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
    fn handle_contract_response(
        shared: &mut Signal<crate::components::shared_state::SharedState>,
        response: ContractResponse,
    ) {
        match response {
            ContractResponse::GetResponse { state, .. } => {
                let bytes = state.as_ref();
                if bytes.is_empty() {
                    return;
                }
                if let Ok(directory) =
                    serde_json::from_slice::<DirectoryState>(bytes)
                {
                    tracing::info!(
                        "Got directory state: {} entries",
                        directory.entries.len()
                    );
                    shared.write().directory = directory;
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
                    _ => return,
                };
                if bytes.is_empty() {
                    return;
                }
                if let Ok(dir_update) =
                    serde_json::from_slice::<DirectoryState>(bytes)
                {
                    tracing::info!(
                        "Directory update: {} entries",
                        dir_update.entries.len()
                    );
                    shared.write().directory.merge(dir_update);
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
                tracing::warn!("Contract not found: {:?}", instance_id);
            }

            _ => {
                tracing::debug!("Unhandled contract response");
            }
        }
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
