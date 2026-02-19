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
    RemoveProduct {
        product_id: String,
    },
    /// Place an order on a storefront.
    PlaceOrder {
        storefront_name: String,
        product_id: String,
        quantity: u32,
        deposit_tier: String,
    },
    /// Subscribe to a specific storefront's updates.
    SubscribeStorefront {
        supplier_name: String,
    },
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

#[cfg(all(target_family = "wasm", feature = "use-node"))]
async fn node_comms(mut rx: UnboundedReceiver<NodeAction>) {
    use futures::channel::mpsc;
    use futures::{SinkExt, StreamExt};

    use freenet_stdlib::client_api::{ClientError, ClientRequest, HostResponse};

    use super::shared_state::use_shared_state;

    let mut shared = use_shared_state();

    // Connect to the node via WebSocket
    let conn = match web_sys::WebSocket::new(
        "ws://localhost:3001/v1/contract/command?encodingProtocol=native",
    ) {
        Ok(c) => c,
        Err(e) => {
            shared.write().last_error = Some(format!("WebSocket connection failed: {:?}", e));
            return;
        }
    };

    let (mut send_responses, mut host_responses) = mpsc::unbounded();
    let (send_half, mut requests) = mpsc::unbounded::<ClientRequest<'static>>();

    let result_handler = {
        let mut sender = send_responses.clone();
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

    // TODO: Subscribe to directory contract here once we have a known contract key.
    // For now, the directory contract key needs to be configured or discovered.

    // Main event loop: multiplex UI actions and node responses
    loop {
        futures::select! {
            // Handle UI actions
            action = rx.next() => {
                let Some(action) = action else { break };
                tracing::debug!("Node action: {:?}", action);
                // TODO: Convert NodeAction to ContractRequest and send via api
                // This will be implemented as we wire up each action type.
            }

            // Handle node responses
            response = host_responses.next() => {
                let Some(response) = response else { break };
                match response {
                    Ok(HostResponse::ContractResponse(cr)) => {
                        handle_contract_response(&mut shared, cr);
                    }
                    Ok(HostResponse::Ok) => {
                        tracing::debug!("Node OK response");
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

            // Forward queued requests to the WebSocket
            request = requests.next() => {
                let Some(request) = request else { break };
                if let Err(e) = api.send(request).await {
                    tracing::error!("Failed to send request: {:?}", e);
                    shared.write().last_error = Some(format!("Send failed: {:?}", e));
                }
            }
        }
    }

    shared.write().connected = false;
}

#[cfg(all(target_family = "wasm", feature = "use-node"))]
fn handle_contract_response(
    shared: &mut Signal<super::shared_state::SharedState>,
    response: freenet_stdlib::client_api::ContractResponse,
) {
    use freenet_stdlib::client_api::ContractResponse;

    match response {
        ContractResponse::GetResponse { key, state, .. } => {
            let bytes = state.as_ref();
            if bytes.is_empty() {
                return;
            }
            // Try to deserialize as directory state
            if let Ok(directory) =
                serde_json::from_slice::<cream_common::directory::DirectoryState>(bytes)
            {
                tracing::info!("Got directory state with {} entries", directory.entries.len());
                shared.write().directory = directory;
            }
            // Try to deserialize as storefront state
            else if let Ok(storefront) =
                serde_json::from_slice::<cream_common::storefront::StorefrontState>(bytes)
            {
                tracing::info!("Got storefront state: {}", storefront.info.name);
                let name = storefront.info.name.clone();
                shared.write().storefronts.insert(name, storefront);
            }
        }
        ContractResponse::UpdateNotification { key, update } => {
            let bytes = match &update {
                freenet_stdlib::prelude::UpdateData::State(s) => s.as_ref(),
                freenet_stdlib::prelude::UpdateData::Delta(d) => d.as_ref(),
                _ => return,
            };
            if bytes.is_empty() {
                return;
            }
            // Try directory update
            if let Ok(update_dir) =
                serde_json::from_slice::<cream_common::directory::DirectoryState>(bytes)
            {
                tracing::info!(
                    "Directory update: {} entries",
                    update_dir.entries.len()
                );
                shared.write().directory.merge(update_dir);
            }
            // Try storefront update
            else if let Ok(update_sf) =
                serde_json::from_slice::<cream_common::storefront::StorefrontState>(bytes)
            {
                let name = update_sf.info.name.clone();
                let mut state = shared.write();
                if let Some(existing) = state.storefronts.get_mut(&name) {
                    existing.merge(update_sf);
                } else {
                    state.storefronts.insert(name, update_sf);
                }
            }
        }
        ContractResponse::PutResponse { key } => {
            tracing::info!("Contract put successful: {:?}", key);
        }
        ContractResponse::UpdateResponse { key, summary } => {
            tracing::debug!("Contract update acknowledged: {:?}", key);
        }
        ContractResponse::SubscribeResponse { key, subscribed } => {
            tracing::info!("Subscription for {:?}: {}", key, subscribed);
        }
        ContractResponse::NotFound { instance_id } => {
            tracing::warn!("Contract not found: {:?}", instance_id);
        }
        _ => {
            tracing::debug!("Unhandled contract response");
        }
    }
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
