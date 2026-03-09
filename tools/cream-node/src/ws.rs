use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use freenet_stdlib::client_api::{ClientRequest, ContractRequest, ContractResponse, HostResponse};
use freenet_stdlib::prelude::*;
use futures::{SinkExt, StreamExt};
use tokio::sync::broadcast;

use crate::contracts;
use crate::store::ContractRow;
use crate::AppState;

type HostResult = Result<HostResponse, freenet_stdlib::client_api::ClientError>;

pub async fn handle_connection(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Channel for subscription notifications to be forwarded to this connection
    let (sub_notify_tx, mut sub_notify_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        match bincode::deserialize::<ClientRequest>(&data) {
                            Ok(request) => {
                                let should_close = matches!(
                                    request,
                                    ClientRequest::Disconnect { .. } | ClientRequest::Close
                                );
                                let response_bytes = handle_request(
                                    &request,
                                    &state,
                                    &sub_notify_tx,
                                ).await;
                                for bytes in response_bytes {
                                    if ws_tx.send(Message::Binary(bytes.into())).await.is_err() {
                                        return;
                                    }
                                }
                                if should_close {
                                    return;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to deserialize ClientRequest: {e}");
                                let err_resp = make_error_response(&format!("deserialization error: {e}"));
                                if ws_tx.send(Message::Binary(err_resp.into())).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Text(data))) => {
                        match bincode::deserialize::<ClientRequest>(data.as_bytes()) {
                            Ok(request) => {
                                let should_close = matches!(
                                    request,
                                    ClientRequest::Disconnect { .. } | ClientRequest::Close
                                );
                                let response_bytes = handle_request(
                                    &request,
                                    &state,
                                    &sub_notify_tx,
                                ).await;
                                for bytes in response_bytes {
                                    if ws_tx.send(Message::Binary(bytes.into())).await.is_err() {
                                        return;
                                    }
                                }
                                if should_close {
                                    return;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to deserialize ClientRequest from text: {e}");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if ws_tx.send(Message::Pong(data)).await.is_err() {
                            return;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    _ => continue,
                }
            }
            Some(notification_bytes) = sub_notify_rx.recv() => {
                if ws_tx.send(Message::Binary(notification_bytes.into())).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Handle a single ClientRequest and return zero or more response byte vectors.
async fn handle_request(
    request: &ClientRequest<'_>,
    state: &AppState,
    notify_tx: &tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Vec<Vec<u8>> {
    match request {
        ClientRequest::ContractOp(contract_req) => {
            handle_contract_request(contract_req, state, notify_tx).await
        }
        ClientRequest::Disconnect { .. } | ClientRequest::Close => {
            vec![serialize_host_result(&Ok(HostResponse::Ok))]
        }
        ClientRequest::Authenticate { .. } => {
            vec![serialize_host_result(&Ok(HostResponse::Ok))]
        }
        ClientRequest::DelegateOp(_) => {
            tracing::debug!("Delegate operations not supported, returning Ok");
            vec![serialize_host_result(&Ok(HostResponse::Ok))]
        }
        ClientRequest::NodeQueries(_) => {
            tracing::debug!("Node queries not supported, returning Ok");
            vec![serialize_host_result(&Ok(HostResponse::Ok))]
        }
        _ => {
            tracing::debug!("Unsupported request type, returning Ok");
            vec![serialize_host_result(&Ok(HostResponse::Ok))]
        }
    }
}

async fn handle_contract_request(
    request: &ContractRequest<'_>,
    state: &AppState,
    notify_tx: &tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Vec<Vec<u8>> {
    match request {
        ContractRequest::Put {
            contract,
            state: wrapped_state,
            subscribe,
            ..
        } => handle_put(contract, wrapped_state, *subscribe, state, notify_tx).await,

        ContractRequest::Get {
            key,
            return_contract_code,
            subscribe,
            ..
        } => handle_get(key, *return_contract_code, *subscribe, state, notify_tx).await,

        ContractRequest::Update { key, data } => handle_update(key, data, state).await,

        ContractRequest::Subscribe { key, .. } => handle_subscribe(key, state, notify_tx).await,

        _ => {
            tracing::debug!("Unsupported contract request, returning Ok");
            vec![serialize_host_result(&Ok(HostResponse::Ok))]
        }
    }
}

async fn handle_put(
    contract: &ContractContainer,
    wrapped_state: &WrappedState,
    subscribe: bool,
    state: &AppState,
    notify_tx: &tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Vec<Vec<u8>> {
    let contract_key = contract.key();
    let instance_id = contract_key.id().encode();

    // Extract contract components
    let (wasm_code, params_bytes, code_hash) = match contract {
        ContractContainer::Wasm(ContractWasmAPIVersion::V1(wrapped)) => {
            let code = wrapped.data.data().to_vec();
            let params = wrapped.params.as_ref().to_vec();
            let hash = contract_key.code_hash().as_ref().to_vec();
            (code, params, hash)
        }
        _ => {
            tracing::warn!("Unsupported contract container version");
            return vec![make_contract_error_put(&contract_key, "unsupported contract version")];
        }
    };

    let state_bytes = wrapped_state.as_ref().to_vec();

    // Classify the contract type
    let (contract_type, _owner) = contracts::classify(&params_bytes, &state_bytes);

    // Validate initial state (errors acceptable in dev mode)
    match contracts::validate_state(contract_type, &params_bytes, &state_bytes) {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!("PUT validation failed for {instance_id}");
            return vec![make_contract_error_put(&contract_key, "validation failed")];
        }
        Err(e) => {
            tracing::warn!("PUT validation error for {instance_id}: {e}");
        }
    }

    // Check if contract already exists — if so, merge (Freenet PUT semantics)
    let final_state_bytes = match state.store.get_contract(&instance_id).await {
        Ok(Some(existing)) => {
            // Merge new state into existing state
            match contracts::apply_update(
                existing.contract_type,
                &existing.parameters_bytes,
                &existing.state_bytes,
                &state_bytes,
            ) {
                Ok(merged) => {
                    tracing::info!(
                        "PUT-merge contract {instance_id} (type: {:?})",
                        existing.contract_type
                    );
                    merged
                }
                Err(e) => {
                    tracing::warn!(
                        "PUT-merge failed for {instance_id}, using new state: {e}"
                    );
                    state_bytes
                }
            }
        }
        _ => state_bytes,
    };

    // Build JSON for audit trail
    let state_json = serde_json::from_slice::<serde_json::Value>(&final_state_bytes)
        .unwrap_or(serde_json::Value::Null);

    let row = ContractRow {
        contract_instance_id: instance_id.clone(),
        contract_key_bytes: contract_key.as_bytes().to_vec(),
        contract_type,
        parameters_bytes: params_bytes.to_vec(),
        state_bytes: final_state_bytes,
        state_json,
        wasm_code: Some(wasm_code),
        code_hash,
    };

    if let Err(e) = state.store.put_contract(&row).await {
        tracing::error!("PUT failed for {instance_id}: {e}");
        return vec![make_contract_error_put(&contract_key, &e.to_string())];
    }

    tracing::info!("PUT contract {instance_id} (type: {contract_type:?})");

    let mut responses = vec![serialize_host_result(&Ok(
        HostResponse::ContractResponse(ContractResponse::PutResponse {
            key: contract_key.clone(),
        }),
    ))];

    if subscribe {
        spawn_subscription(&instance_id, state, notify_tx);
        responses.push(serialize_host_result(&Ok(
            HostResponse::ContractResponse(ContractResponse::SubscribeResponse {
                key: contract_key,
                subscribed: true,
            }),
        )));
    }

    responses
}

async fn handle_get(
    key: &ContractInstanceId,
    return_contract_code: bool,
    subscribe: bool,
    state: &AppState,
    notify_tx: &tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Vec<Vec<u8>> {
    let instance_id = key.encode();

    match state.store.get_contract(&instance_id).await {
        Ok(Some(row)) => {
            let wasm = row.wasm_code.clone().unwrap_or_default();
            let contract_key = ContractKey::from_id_and_code(*key, CodeHash::from_code(&wasm));

            let contract_container = if return_contract_code {
                row.wasm_code.as_ref().map(|code| {
                    let code = ContractCode::from(code.clone());
                    let params = Parameters::from(row.parameters_bytes.clone());
                    let wrapped = WrappedContract::new(Arc::new(code), params);
                    ContractContainer::Wasm(ContractWasmAPIVersion::V1(wrapped))
                })
            } else {
                None
            };

            let wrapped_state = WrappedState::new(row.state_bytes);

            let mut responses = vec![serialize_host_result(&Ok(
                HostResponse::ContractResponse(ContractResponse::GetResponse {
                    key: contract_key.clone(),
                    contract: contract_container,
                    state: wrapped_state,
                }),
            ))];

            if subscribe {
                spawn_subscription(&instance_id, state, notify_tx);
                responses.push(serialize_host_result(&Ok(
                    HostResponse::ContractResponse(ContractResponse::SubscribeResponse {
                        key: contract_key,
                        subscribed: true,
                    }),
                )));
            }

            responses
        }
        Ok(None) => {
            tracing::debug!("GET: contract not found: {instance_id}");
            vec![serialize_host_result(&Ok(
                HostResponse::ContractResponse(ContractResponse::NotFound {
                    instance_id: *key,
                }),
            ))]
        }
        Err(e) => {
            tracing::error!("GET failed for {instance_id}: {e}");
            vec![make_error_response(&format!("database error: {e}"))]
        }
    }
}

async fn handle_update(
    key: &ContractKey,
    data: &UpdateData<'_>,
    state: &AppState,
) -> Vec<Vec<u8>> {
    let instance_id = key.id().encode();

    // Extract update bytes from UpdateData
    let update_bytes = match data {
        UpdateData::State(s) => s.as_ref().to_vec(),
        UpdateData::Delta(d) => d.as_ref().to_vec(),
        UpdateData::StateAndDelta { state, .. } => state.as_ref().to_vec(),
        UpdateData::RelatedState { state, .. } => state.as_ref().to_vec(),
        UpdateData::RelatedDelta { delta, .. } => delta.as_ref().to_vec(),
        UpdateData::RelatedStateAndDelta { state, .. } => state.as_ref().to_vec(),
    };

    // Atomically: lock row → merge → persist → audit
    let (row, new_state_bytes) = match state
        .store
        .get_and_update_contract(&instance_id, |row| {
            contracts::apply_update(
                row.contract_type,
                &row.parameters_bytes,
                &row.state_bytes,
                &update_bytes,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .await
    {
        Ok(result) => result,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("contract not found") {
                tracing::warn!("UPDATE: contract not found: {instance_id}");
            } else {
                tracing::warn!("UPDATE failed for {instance_id}: {msg}");
            }
            return vec![make_contract_error_update(key, &msg)];
        }
    };

    tracing::info!("UPDATE contract {instance_id} (type: {:?})", row.contract_type);

    // Build UpdateNotification for subscribers
    let notification = HostResponse::ContractResponse(ContractResponse::UpdateNotification {
        key: key.clone(),
        update: UpdateData::State(State::from(new_state_bytes)),
    });
    let notification_bytes = serialize_host_result(&Ok(notification));
    state.subscriptions.notify(&instance_id, notification_bytes);

    // Build summary (empty — cream-node doesn't use delta sync)
    let summary = StateSummary::from(vec![]);

    vec![serialize_host_result(&Ok(
        HostResponse::ContractResponse(ContractResponse::UpdateResponse {
            key: key.clone(),
            summary,
        }),
    ))]
}

async fn handle_subscribe(
    key: &ContractInstanceId,
    state: &AppState,
    notify_tx: &tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Vec<Vec<u8>> {
    let instance_id = key.encode();

    // Check contract exists to get proper ContractKey
    let contract_key = match state.store.get_contract(&instance_id).await {
        Ok(Some(row)) => {
            let wasm = row.wasm_code.unwrap_or_default();
            ContractKey::from_id_and_code(*key, CodeHash::from_code(&wasm))
        }
        _ => {
            // Contract not found — subscribe anyway (Freenet behavior)
            ContractKey::from_id_and_code(*key, CodeHash::from_code(&[]))
        }
    };

    spawn_subscription(&instance_id, state, notify_tx);

    vec![serialize_host_result(&Ok(
        HostResponse::ContractResponse(ContractResponse::SubscribeResponse {
            key: contract_key,
            subscribed: true,
        }),
    ))]
}

/// Spawn a background task that forwards subscription notifications to the connection.
fn spawn_subscription(
    instance_id: &str,
    state: &AppState,
    notify_tx: &tokio::sync::mpsc::Sender<Vec<u8>>,
) {
    let mut rx = state.subscriptions.subscribe(instance_id);
    let tx = notify_tx.clone();
    let id = instance_id.to_string();

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(bytes) => {
                    if tx.send(bytes).await.is_err() {
                        tracing::debug!("Subscription for {id}: connection closed");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Subscription for {id}: lagged by {n} messages");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::debug!("Subscription for {id}: channel closed");
                    break;
                }
            }
        }
    });
}

fn serialize_host_result(result: &HostResult) -> Vec<u8> {
    bincode::serialize(result).expect("bincode serialization should not fail")
}

fn make_error_response(msg: &str) -> Vec<u8> {
    use freenet_stdlib::client_api::ClientError;
    let err = ClientError::from(msg.to_string());
    bincode::serialize(&Err::<HostResponse, _>(err)).expect("bincode serialization should not fail")
}

fn make_contract_error_put(key: &ContractKey, cause: &str) -> Vec<u8> {
    use freenet_stdlib::client_api::ClientError;
    let err = ClientError::from(format!(
        "put error for {}: {cause}",
        key.encoded_contract_id()
    ));
    bincode::serialize(&Err::<HostResponse, _>(err)).expect("bincode serialization should not fail")
}

fn make_contract_error_update(key: &ContractKey, cause: &str) -> Vec<u8> {
    use freenet_stdlib::client_api::ClientError;
    let err = ClientError::from(format!(
        "update error for {}: {cause}",
        key.encoded_contract_id()
    ));
    bincode::serialize(&Err::<HostResponse, _>(err)).expect("bincode serialization should not fail")
}
