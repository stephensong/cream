//! CREAM FROST guardian daemon.
//!
//! Holds one FROST key share and participates in distributed threshold signing
//! via HTTP. Supports three startup modes:
//!
//! 1. **Keys on disk** → load and activate immediately
//! 2. **`--peers` provided, no keys** → run DKG ceremony with peer guardians
//! 3. **No `--peers`, no keys** → `dev_root_frost_keys()` fallback (trusted dealer)
//!
//! Optionally connects to a co-located Freenet node (`--node-url`) and subscribes
//! to critical contracts (directory, root user) to strengthen replication.

mod contracts;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::Method;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest, ContractResponse, HostResponse};
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::{Any, CorsLayer};

/// TTL for stored nonces (seconds). Expired nonces are cleaned on each round1 call.
const NONCE_TTL_SECS: u64 = 30;

#[derive(Parser)]
#[command(name = "cream-guardian", about = "CREAM FROST guardian daemon")]
struct Cli {
    /// Guardian share index (1-based).
    #[arg(long, default_value_t = 1)]
    share_index: u16,

    /// HTTP port to listen on (default: 3009 + share_index).
    #[arg(long)]
    port: Option<u16>,

    /// Total guardians for initial DKG (default: 3).
    #[arg(long, default_value_t = 3)]
    max_signers: u16,

    /// Signing threshold for initial DKG (default: 2).
    #[arg(long, default_value_t = 2)]
    min_signers: u16,

    /// Comma-separated peer guardian URLs for DKG
    /// (e.g. "http://localhost:3011,http://localhost:3012").
    #[arg(long, value_delimiter = ',')]
    peers: Vec<String>,

    /// WebSocket URL of the co-located Freenet node.
    /// (e.g. "ws://localhost:3005/v1/contract/command?encodingProtocol=native")
    #[arg(long)]
    node_url: Option<String>,

    /// Trigger proactive refresh with existing guardians (requires --peers + keys on disk).
    #[arg(long)]
    refresh: bool,

    /// Act as re-deal coordinator (requires --old-peers, --peers, --new-max-signers, --new-min-signers).
    #[arg(long)]
    redeal: bool,

    /// Quorum of old guardians to collect shares from (for --redeal).
    #[arg(long, value_delimiter = ',')]
    old_peers: Vec<String>,

    /// Total guardians in new set (for --redeal).
    #[arg(long)]
    new_max_signers: Option<u16>,

    /// New threshold (for --redeal).
    #[arg(long)]
    new_min_signers: Option<u16>,
}

struct AppState {
    identifier: frost::Identifier,
    share_index: u16,
    max_signers: std::sync::atomic::AtomicU16,
    min_signers: std::sync::atomic::AtomicU16,
    key_package: RwLock<Option<frost::keys::KeyPackage>>,
    public_key_package: RwLock<Option<frost::keys::PublicKeyPackage>>,
    nonces: Mutex<BTreeMap<String, (frost::round1::SigningNonces, Instant)>>,
    dkg_state: Mutex<DkgState>,
    refresh_state: Mutex<DkgState>,
    refreshing: AtomicBool,
    node_connected: AtomicBool,
}

impl AppState {
    fn is_ready(&self) -> bool {
        if self.refreshing.load(Ordering::Relaxed) {
            return false;
        }
        // Use try_read to avoid blocking — if locked, not ready yet
        self.key_package
            .try_read()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }
}

/// Collects DKG round packages from peers during the ceremony.
#[derive(Default)]
struct DkgState {
    round1_packages: BTreeMap<frost::Identifier, frost::keys::dkg::round1::Package>,
    round2_packages: BTreeMap<frost::Identifier, frost::keys::dkg::round2::Package>,
}

// ─── API types ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Round1Request {
    session_id: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Round1Response {
    pub identifier: frost::Identifier,
    pub commitments: frost::round1::SigningCommitments,
}

#[derive(Deserialize)]
struct Round2Request {
    session_id: String,
    message_hex: String,
    /// Commitments from all participating guardians (collected by the coordinator).
    signing_commitments: Vec<Round1Response>,
}

#[derive(Serialize)]
struct Round2Response {
    identifier: frost::Identifier,
    signature_share: frost::round2::SignatureShare,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    identifier: frost::Identifier,
    ready: bool,
    refreshing: bool,
    node_connected: bool,
}

#[derive(Serialize)]
struct ConfigResponse {
    min_signers: u16,
    max_signers: u16,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ─── Redeal API types ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct RedealShareResponse {
    key_package: frost::keys::KeyPackage,
}

#[derive(Serialize, Deserialize)]
struct RedealReceiveRequest {
    secret_share: frost::keys::SecretShare,
    public_key_package: frost::keys::PublicKeyPackage,
}

// ─── DKG API types ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct DkgRound1Request {
    identifier: frost::Identifier,
    package: frost::keys::dkg::round1::Package,
}

#[derive(Serialize, Deserialize)]
struct DkgRound2Request {
    from: frost::Identifier,
    package: frost::keys::dkg::round2::Package,
}

#[derive(Serialize, Deserialize)]
struct DkgRoundResponse {
    ok: bool,
}

// ─── Signing Handlers ───────────────────────────────────────────────────────

async fn round1_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<Round1Request>,
) -> Result<Json<Round1Response>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    use rand::rngs::OsRng;

    let key_package_guard = state.key_package.read().await;
    let key_package = key_package_guard.as_ref().ok_or_else(|| {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Guardian not ready (DKG in progress)".to_string(),
            }),
        )
    })?;

    let (nonces, commitments) = frost::round1::commit(key_package.signing_share(), &mut OsRng);

    let mut nonce_map = state.nonces.lock().await;
    // Clean expired nonces
    nonce_map.retain(|_, (_, created)| created.elapsed().as_secs() < NONCE_TTL_SECS);
    nonce_map.insert(req.session_id, (nonces, Instant::now()));

    Ok(Json(Round1Response {
        identifier: state.identifier,
        commitments,
    }))
}

async fn round2_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<Round2Request>,
) -> Result<Json<Round2Response>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let key_package_guard = state.key_package.read().await;
    let key_package = key_package_guard.as_ref().ok_or_else(|| {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Guardian not ready (DKG in progress)".to_string(),
            }),
        )
    })?;

    // Retrieve and consume stored nonces
    let nonces = {
        let mut nonce_map = state.nonces.lock().await;
        match nonce_map.remove(&req.session_id) {
            Some((nonces, _)) => nonces,
            None => {
                return Err((
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("No nonces found for session_id '{}'", req.session_id),
                    }),
                ));
            }
        }
    };

    // Decode the message
    let message = match hex::decode(&req.message_hex) {
        Ok(m) => m,
        Err(e) => {
            return Err((
                axum::http::StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid message_hex: {}", e),
                }),
            ));
        }
    };

    // Build commitments map
    let commitments_map: BTreeMap<frost::Identifier, frost::round1::SigningCommitments> = req
        .signing_commitments
        .into_iter()
        .map(|c| (c.identifier, c.commitments))
        .collect();

    // Create signing package and sign
    let signing_package = frost::SigningPackage::new(commitments_map, &message);
    let signature_share =
        frost::round2::sign(&signing_package, &nonces, key_package).map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("FROST round2 signing failed: {}", e),
                }),
            )
        })?;

    Ok(Json(Round2Response {
        identifier: state.identifier,
        signature_share,
    }))
}

// ─── DKG Handlers ───────────────────────────────────────────────────────────

async fn dkg_round1_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DkgRound1Request>,
) -> Json<DkgRoundResponse> {
    let mut dkg = state.dkg_state.lock().await;
    println!(
        "  DKG: received round1 package from {:?}",
        req.identifier
    );
    dkg.round1_packages.insert(req.identifier, req.package);
    Json(DkgRoundResponse { ok: true })
}

async fn dkg_round2_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DkgRound2Request>,
) -> Json<DkgRoundResponse> {
    let mut dkg = state.dkg_state.lock().await;
    println!("  DKG: received round2 package from {:?}", req.from);
    dkg.round2_packages.insert(req.from, req.package);
    Json(DkgRoundResponse { ok: true })
}

async fn public_key_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<frost::keys::PublicKeyPackage>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let guard = state.public_key_package.read().await;
    match guard.as_ref() {
        Some(pkg) => Ok(Json(pkg.clone())),
        None => Err((
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Guardian not ready (DKG in progress)".to_string(),
            }),
        )),
    }
}

// ─── Health ─────────────────────────────────────────────────────────────────

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        identifier: state.identifier,
        ready: state.is_ready(),
        refreshing: state.refreshing.load(Ordering::Relaxed),
        node_connected: state.node_connected.load(Ordering::Relaxed),
    })
}

async fn config_handler(State(state): State<Arc<AppState>>) -> Json<ConfigResponse> {
    Json(ConfigResponse {
        min_signers: state.min_signers.load(Ordering::Relaxed),
        max_signers: state.max_signers.load(Ordering::Relaxed),
    })
}

// ─── Key Persistence ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct PersistedKeys {
    key_package: frost::keys::KeyPackage,
    public_key_package: frost::keys::PublicKeyPackage,
}

fn keys_path(share_index: u16) -> PathBuf {
    let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    cache
        .join("freenet")
        .join(format!("guardian-{}", share_index))
        .join("frost-dkg.json")
}

fn load_keys(share_index: u16) -> Option<PersistedKeys> {
    let path = keys_path(share_index);
    let data = std::fs::read_to_string(&path).ok()?;
    let keys: PersistedKeys = serde_json::from_str(&data).ok()?;
    println!("Loaded DKG keys from {}", path.display());
    Some(keys)
}

fn save_keys(share_index: u16, keys: &PersistedKeys) -> Result<(), String> {
    let path = keys_path(share_index);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
    }
    let data =
        serde_json::to_string_pretty(keys).map_err(|e| format!("Failed to serialize: {}", e))?;
    std::fs::write(&path, data).map_err(|e| format!("Failed to write: {}", e))?;
    println!("Saved DKG keys to {}", path.display());
    Ok(())
}

// ─── DKG Ceremony ───────────────────────────────────────────────────────────

async fn run_dkg(state: Arc<AppState>, peers: Vec<String>) {
    use rand::rngs::OsRng;

    let max_signers = state.max_signers.load(Ordering::Relaxed);
    let min_signers = state.min_signers.load(Ordering::Relaxed);
    let n_peers = peers.len();
    println!(
        "DKG: starting ceremony with {} peers (total {} guardians, threshold {})",
        n_peers,
        n_peers + 1,
        min_signers
    );

    // Wait for all peers to be healthy
    let client = reqwest::Client::new();
    for peer in &peers {
        println!("DKG: waiting for peer {} ...", peer);
        loop {
            match client.get(format!("{}/health", peer)).send().await {
                Ok(resp) if resp.status().is_success() => break,
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
        println!("DKG: peer {} is up", peer);
    }

    // ── Round 1 ──
    println!("DKG: executing round 1...");
    let (round1_secret, round1_package) = frost::keys::dkg::part1(
        state.identifier,
        max_signers,
        min_signers,
        &mut OsRng,
    )
    .expect("DKG part1 should not fail");

    // Send our round1 package to all peers
    for peer in &peers {
        let req = DkgRound1Request {
            identifier: state.identifier,
            package: round1_package.clone(),
        };
        client
            .post(format!("{}/dkg/round1", peer))
            .json(&req)
            .send()
            .await
            .unwrap_or_else(|e| panic!("DKG: failed to send round1 to {}: {}", peer, e));
    }

    // Wait for round1 packages from all peers
    let expected = n_peers;
    loop {
        let count = state.dkg_state.lock().await.round1_packages.len();
        if count >= expected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let round1_packages = {
        let dkg = state.dkg_state.lock().await;
        dkg.round1_packages.clone()
    };
    println!(
        "DKG: round 1 complete — received {} peer packages",
        round1_packages.len()
    );

    // ── Round 2 ──
    println!("DKG: executing round 2...");
    let (round2_secret, round2_packages) =
        frost::keys::dkg::part2(round1_secret, &round1_packages)
            .expect("DKG part2 should not fail");

    // Send per-recipient round2 packages to each peer
    // round2_packages is BTreeMap<Identifier, round2::Package> — one per peer
    for (recipient_id, package) in &round2_packages {
        // Find the peer URL for this identifier
        // Peers are ordered, identifiers are 1-based: our index is state.share_index,
        // so we need to map identifier → peer URL
        let peer_url = identifier_to_peer_url(*recipient_id, state.share_index, max_signers, &peers);
        if let Some(url) = peer_url {
            let req = DkgRound2Request {
                from: state.identifier,
                package: package.clone(),
            };
            client
                .post(format!("{}/dkg/round2", url))
                .json(&req)
                .send()
                .await
                .unwrap_or_else(|e| panic!("DKG: failed to send round2 to {}: {}", url, e));
        }
    }

    // Wait for round2 packages from all peers
    loop {
        let count = state.dkg_state.lock().await.round2_packages.len();
        if count >= expected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let round2_received = {
        let dkg = state.dkg_state.lock().await;
        dkg.round2_packages.clone()
    };
    println!(
        "DKG: round 2 complete — received {} peer packages",
        round2_received.len()
    );

    // ── Round 3 (finalize) ──
    println!("DKG: finalizing...");
    let (key_package, public_key_package) =
        frost::keys::dkg::part3(&round2_secret, &round1_packages, &round2_received)
            .expect("DKG part3 should not fail");

    // Persist keys
    let persisted = PersistedKeys {
        key_package: key_package.clone(),
        public_key_package: public_key_package.clone(),
    };
    save_keys(state.share_index, &persisted).expect("Failed to save DKG keys");

    // Activate
    *state.key_package.write().await = Some(key_package);
    *state.public_key_package.write().await = Some(public_key_package);

    println!("DKG: ceremony complete — guardian is ready for signing");
}

// ─── Refresh Handlers ────────────────────────────────────────────────────────

async fn refresh_round1_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DkgRound1Request>,
) -> Json<DkgRoundResponse> {
    let mut refresh = state.refresh_state.lock().await;
    println!(
        "  Refresh: received round1 package from {:?}",
        req.identifier
    );
    refresh.round1_packages.insert(req.identifier, req.package);
    Json(DkgRoundResponse { ok: true })
}

async fn refresh_round2_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DkgRound2Request>,
) -> Json<DkgRoundResponse> {
    let mut refresh = state.refresh_state.lock().await;
    println!("  Refresh: received round2 package from {:?}", req.from);
    refresh.round2_packages.insert(req.from, req.package);
    Json(DkgRoundResponse { ok: true })
}

// ─── Redeal Handlers ─────────────────────────────────────────────────────────

async fn redeal_share_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RedealShareResponse>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let key_package_guard = state.key_package.read().await;
    let key_package = key_package_guard.as_ref().ok_or_else(|| {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Guardian not ready (no keys)".to_string(),
            }),
        )
    })?;
    Ok(Json(RedealShareResponse {
        key_package: key_package.clone(),
    }))
}

async fn redeal_receive_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RedealReceiveRequest>,
) -> Result<Json<DkgRoundResponse>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let key_package =
        frost::keys::KeyPackage::try_from(req.secret_share).map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid secret share: {}", e),
                }),
            )
        })?;

    let new_max = req.public_key_package.verifying_shares().len() as u16;
    let new_min = *key_package.min_signers();

    // Persist new keys
    let persisted = PersistedKeys {
        key_package: key_package.clone(),
        public_key_package: req.public_key_package.clone(),
    };
    save_keys(state.share_index, &persisted).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to save keys: {}", e),
            }),
        )
    })?;

    // Activate new keys
    *state.key_package.write().await = Some(key_package);
    *state.public_key_package.write().await = Some(req.public_key_package);
    state.max_signers.store(new_max, Ordering::Relaxed);
    state.min_signers.store(new_min, Ordering::Relaxed);
    state.refreshing.store(false, Ordering::Relaxed);

    println!(
        "Redeal: received and activated new keys ({}-of-{})",
        new_min, new_max
    );

    Ok(Json(DkgRoundResponse { ok: true }))
}

// ─── Proactive Refresh Ceremony ──────────────────────────────────────────────

async fn run_refresh(state: Arc<AppState>, peers: Vec<String>) {
    use frost::keys::refresh;
    use rand::rngs::OsRng;

    state.refreshing.store(true, Ordering::Relaxed);

    let max_signers = state.max_signers.load(Ordering::Relaxed);
    let min_signers = state.min_signers.load(Ordering::Relaxed);

    let old_key_package = state
        .key_package
        .read()
        .await
        .clone()
        .expect("Refresh requires keys on disk");
    let old_pub_key_package = state
        .public_key_package
        .read()
        .await
        .clone()
        .expect("Refresh requires keys on disk");

    println!(
        "Refresh: starting proactive refresh with {} peers",
        peers.len()
    );

    // Wait for all peers to be healthy
    let client = reqwest::Client::new();
    for peer in &peers {
        println!("Refresh: waiting for peer {} ...", peer);
        loop {
            match client.get(format!("{}/health", peer)).send().await {
                Ok(resp) if resp.status().is_success() => break,
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
        println!("Refresh: peer {} is up", peer);
    }

    // ── Round 1 ──
    println!("Refresh: executing round 1...");
    let (round1_secret, round1_package) =
        refresh::refresh_dkg_part1(state.identifier, max_signers, min_signers, &mut OsRng)
            .expect("Refresh part1 should not fail");

    // Send our round1 package to all peers
    for peer in &peers {
        let req = DkgRound1Request {
            identifier: state.identifier,
            package: round1_package.clone(),
        };
        client
            .post(format!("{}/refresh/round1", peer))
            .json(&req)
            .send()
            .await
            .unwrap_or_else(|e| panic!("Refresh: failed to send round1 to {}: {}", peer, e));
    }

    // Wait for round1 packages from all peers
    let expected = peers.len();
    loop {
        let count = state.refresh_state.lock().await.round1_packages.len();
        if count >= expected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let round1_packages = {
        let refresh = state.refresh_state.lock().await;
        refresh.round1_packages.clone()
    };
    println!(
        "Refresh: round 1 complete — received {} peer packages",
        round1_packages.len()
    );

    // ── Round 2 ──
    println!("Refresh: executing round 2...");
    let (round2_secret, round2_packages) =
        refresh::refresh_dkg_part2(round1_secret, &round1_packages)
            .expect("Refresh part2 should not fail");

    // Send per-recipient round2 packages
    for (recipient_id, package) in &round2_packages {
        let peer_url =
            identifier_to_peer_url(*recipient_id, state.share_index, max_signers, &peers);
        if let Some(url) = peer_url {
            let req = DkgRound2Request {
                from: state.identifier,
                package: package.clone(),
            };
            client
                .post(format!("{}/refresh/round2", url))
                .json(&req)
                .send()
                .await
                .unwrap_or_else(|e| panic!("Refresh: failed to send round2 to {}: {}", url, e));
        }
    }

    // Wait for round2 packages from all peers
    loop {
        let count = state.refresh_state.lock().await.round2_packages.len();
        if count >= expected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let round2_received = {
        let refresh = state.refresh_state.lock().await;
        refresh.round2_packages.clone()
    };
    println!(
        "Refresh: round 2 complete — received {} peer packages",
        round2_received.len()
    );

    // ── Finalize ──
    println!("Refresh: finalizing...");
    let (key_package, public_key_package) = refresh::refresh_dkg_shares(
        &round2_secret,
        &round1_packages,
        &round2_received,
        old_pub_key_package,
        old_key_package,
    )
    .expect("Refresh finalize should not fail");

    // Persist new keys
    let persisted = PersistedKeys {
        key_package: key_package.clone(),
        public_key_package: public_key_package.clone(),
    };
    save_keys(state.share_index, &persisted).expect("Failed to save refreshed keys");

    // Activate
    *state.key_package.write().await = Some(key_package);
    *state.public_key_package.write().await = Some(public_key_package);
    state.refreshing.store(false, Ordering::Relaxed);

    println!("Refresh: proactive refresh complete — guardian is ready for signing");
}

// ─── Re-deal Ceremony ────────────────────────────────────────────────────────

async fn run_redeal(
    state: Arc<AppState>,
    old_peers: Vec<String>,
    new_peers: Vec<String>,
    new_max_signers: u16,
    new_min_signers: u16,
) {
    use rand::rngs::OsRng;

    state.refreshing.store(true, Ordering::Relaxed);

    println!(
        "Redeal: coordinator starting — collecting shares from {} old peers, splitting to {}-of-{}",
        old_peers.len(),
        new_min_signers,
        new_max_signers
    );

    let client = reqwest::Client::new();

    // Collect KeyPackages from old peers (including self)
    let mut key_packages: Vec<frost::keys::KeyPackage> = Vec::new();

    // Add our own key package
    let own_key = state
        .key_package
        .read()
        .await
        .clone()
        .expect("Redeal coordinator must have keys");
    key_packages.push(own_key);

    let old_pub_key_package = state
        .public_key_package
        .read()
        .await
        .clone()
        .expect("Redeal coordinator must have public key");

    // Collect from old peers
    let min_signers = state.min_signers.load(Ordering::Relaxed);
    for peer in &old_peers {
        println!("Redeal: collecting key share from {} ...", peer);
        let resp = client
            .post(format!("{}/redeal/share", peer))
            .send()
            .await
            .unwrap_or_else(|e| panic!("Redeal: failed to contact {}: {}", peer, e));

        let share_resp: RedealShareResponse = resp
            .json()
            .await
            .unwrap_or_else(|e| panic!("Redeal: failed to parse share from {}: {}", peer, e));
        key_packages.push(share_resp.key_package);

        // We only need min_signers shares for reconstruction
        if key_packages.len() >= min_signers as usize {
            break;
        }
    }

    println!(
        "Redeal: collected {} key packages (need {} for reconstruction)",
        key_packages.len(),
        min_signers
    );

    // Reconstruct the group secret
    let signing_key = frost::keys::reconstruct(&key_packages)
        .expect("Reconstruction should not fail with sufficient shares");

    // Verify reconstructed key matches group verifying key
    let expected_vk = old_pub_key_package.verifying_key();
    let reconstructed_vk = signing_key.into();
    assert_eq!(
        *expected_vk, reconstructed_vk,
        "Redeal: reconstructed key does not match group verifying key!"
    );
    println!("Redeal: reconstruction verified — group key matches");

    // Build identifier list for new set
    let identifiers: Vec<frost::Identifier> = (1..=new_max_signers)
        .map(|i| frost::Identifier::try_from(i).expect("valid identifier"))
        .collect();

    // Split into new shares
    let (new_shares, new_pub_key_package) = frost::keys::split(
        &signing_key,
        new_max_signers,
        new_min_signers,
        frost::keys::IdentifierList::Custom(&identifiers),
        &mut OsRng,
    )
    .expect("Split should not fail");

    // Zeroize signing key by dropping it (signing_key consumed above; the binding
    // `signing_key` is already moved into `split`).

    println!(
        "Redeal: split complete — distributing {} shares to new guardians",
        new_shares.len()
    );

    // Wait for all new peers to be healthy before distributing
    for peer in &new_peers {
        println!("Redeal: waiting for new peer {} ...", peer);
        loop {
            match client.get(format!("{}/health", peer)).send().await {
                Ok(resp) if resp.status().is_success() => break,
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
        println!("Redeal: new peer {} is up", peer);
    }

    // Distribute shares to new peers
    // new_peers are all guardians except ourselves in the new set
    for (id, share) in &new_shares {
        if *id == state.identifier {
            // This is our own new share — activate directly
            let key_package = frost::keys::KeyPackage::try_from(share.clone())
                .expect("KeyPackage conversion should not fail");
            let persisted = PersistedKeys {
                key_package: key_package.clone(),
                public_key_package: new_pub_key_package.clone(),
            };
            save_keys(state.share_index, &persisted).expect("Failed to save re-dealt keys");
            *state.key_package.write().await = Some(key_package);
            *state.public_key_package.write().await = Some(new_pub_key_package.clone());
            state
                .max_signers
                .store(new_max_signers, Ordering::Relaxed);
            state
                .min_signers
                .store(new_min_signers, Ordering::Relaxed);
            println!("Redeal: activated own new share");
            continue;
        }

        // Find the peer URL for this identifier
        let peer_url =
            identifier_to_peer_url(*id, state.share_index, new_max_signers, &new_peers);
        if let Some(url) = peer_url {
            let req = RedealReceiveRequest {
                secret_share: share.clone(),
                public_key_package: new_pub_key_package.clone(),
            };
            client
                .post(format!("{}/redeal/receive", url))
                .json(&req)
                .send()
                .await
                .unwrap_or_else(|e| panic!("Redeal: failed to send share to {}: {}", url, e));
            println!("Redeal: sent share to {}", url);
        }
    }

    state.refreshing.store(false, Ordering::Relaxed);
    println!(
        "Redeal: complete — new federation is {}-of-{}",
        new_min_signers, new_max_signers
    );
}

// ─── Contract Monitoring ─────────────────────────────────────────────────────

/// Connect to the co-located Freenet node and subscribe to critical contracts.
///
/// Waits for signing readiness (keys loaded/DKG complete), then connects via
/// WebSocket and subscribes to the directory and root user contracts. Runs an
/// event loop logging `UpdateNotification` events. Reconnects on disconnect.
async fn monitor_contracts(state: Arc<AppState>, node_url: String) {
    // Wait until keys are ready (DKG may still be running)
    loop {
        if state.is_ready() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let pubkey_package = state
        .public_key_package
        .read()
        .await
        .clone()
        .expect("public key package must be set when ready");

    let directory_key = contracts::directory_contract_key();
    let root_user_key = contracts::root_user_contract_key(&pubkey_package);

    println!(
        "Node monitor: directory contract key = {}",
        directory_key
    );
    println!(
        "Node monitor: root user contract key = {}",
        root_user_key
    );

    let mut backoff = std::time::Duration::from_secs(1);
    let max_backoff = std::time::Duration::from_secs(30);

    loop {
        println!("Node monitor: connecting to {} ...", node_url);

        let ws_conn = match tokio_tungstenite::connect_async(&node_url).await {
            Ok((conn, _)) => conn,
            Err(e) => {
                println!("Node monitor: WebSocket connect failed: {} (retrying in {:?})", e, backoff);
                state.node_connected.store(false, Ordering::Relaxed);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
                continue;
            }
        };

        let mut api = freenet_stdlib::client_api::WebApi::start(ws_conn);

        // Subscribe to directory contract
        if let Err(e) = api
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *directory_key.id(),
                summary: None,
            }))
            .await
        {
            println!("Node monitor: failed to subscribe to directory: {}", e);
            state.node_connected.store(false, Ordering::Relaxed);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
            continue;
        }
        println!("Node monitor: subscribed to directory contract");

        // Subscribe to root user contract
        if let Err(e) = api
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *root_user_key.id(),
                summary: None,
            }))
            .await
        {
            println!("Node monitor: failed to subscribe to root user contract: {}", e);
            state.node_connected.store(false, Ordering::Relaxed);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
            continue;
        }
        println!("Node monitor: subscribed to root user contract");

        state.node_connected.store(true, Ordering::Relaxed);
        backoff = std::time::Duration::from_secs(1); // Reset backoff on success
        println!("Node monitor: connected and subscribed — listening for updates");

        // Event loop: log notifications
        loop {
            match api.recv().await {
                Ok(HostResponse::ContractResponse(ContractResponse::UpdateNotification {
                    key,
                    ..
                })) => {
                    println!("Node monitor: update notification for contract {}", key);
                }
                Ok(HostResponse::Ok) => {
                    // Subscription acknowledgement or other OK — ignore
                }
                Ok(other) => {
                    println!("Node monitor: received {:?}", other);
                }
                Err(e) => {
                    println!("Node monitor: connection error: {} — reconnecting", e);
                    state.node_connected.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

/// Map a FROST Identifier to its peer URL.
///
/// Peer URLs are ordered by their share index, excluding our own index.
/// FROST identifiers are 1-based u16: 1, 2, 3.
/// If our share_index is 2 and peers are ["http://...:3010", "http://...:3012"],
/// then peer indices are [1, 3] (all indices except 2).
fn identifier_to_peer_url(
    target: frost::Identifier,
    our_share_index: u16,
    max_signers: u16,
    peers: &[String],
) -> Option<String> {
    // Build the list of peer share indices (all valid indices except ours)
    let peer_indices: Vec<u16> = (1..=max_signers)
        .filter(|&i| i != our_share_index)
        .collect();

    // Find which peer index matches the target identifier
    for (i, &idx) in peer_indices.iter().enumerate() {
        let id = frost::Identifier::try_from(idx).ok()?;
        if id == target {
            return peers.get(i).cloned();
        }
    }
    None
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let identifier = frost::Identifier::try_from(cli.share_index)
        .expect("Invalid share_index (must be 1..=max_signers)");

    let port = cli.port.unwrap_or(3009 + cli.share_index);

    let state = Arc::new(AppState {
        identifier,
        share_index: cli.share_index,
        max_signers: std::sync::atomic::AtomicU16::new(cli.max_signers),
        min_signers: std::sync::atomic::AtomicU16::new(cli.min_signers),
        key_package: RwLock::new(None),
        public_key_package: RwLock::new(None),
        nonces: Mutex::new(BTreeMap::new()),
        dkg_state: Mutex::new(DkgState::default()),
        refresh_state: Mutex::new(DkgState::default()),
        refreshing: AtomicBool::new(false),
        node_connected: AtomicBool::new(false),
    });

    // ── Key initialization ──
    if let Some(persisted) = load_keys(cli.share_index) {
        // Derive max/min from loaded keys
        let loaded_min = *persisted.key_package.min_signers();
        let loaded_max = persisted.public_key_package.verifying_shares().len() as u16;
        state.max_signers.store(loaded_max, Ordering::Relaxed);
        state.min_signers.store(loaded_min, Ordering::Relaxed);

        // Mode 1: Keys on disk → activate immediately
        *state.key_package.write().await = Some(persisted.key_package);
        *state.public_key_package.write().await = Some(persisted.public_key_package);
        println!(
            "Guardian {} ready (loaded from disk, {}-of-{})",
            cli.share_index, loaded_min, loaded_max
        );
    } else if !cli.peers.is_empty() && !cli.refresh && !cli.redeal {
        // Mode 2: --peers provided → DKG will run after server starts
        println!(
            "Guardian {} will run DKG with {} peers ({}-of-{})",
            cli.share_index,
            cli.peers.len(),
            cli.min_signers,
            cli.max_signers
        );
    } else if !cli.refresh && !cli.redeal {
        // Mode 3: No peers, no keys → trusted dealer fallback (or wait if index out of range)
        let (key_packages, pubkey_package) = cream_common::frost::dev_root_frost_keys();
        if let Some(key_package) = key_packages.get(&identifier) {
            *state.key_package.write().await = Some(key_package.clone());
            *state.public_key_package.write().await = Some(pubkey_package);
            println!(
                "Guardian {} ready (trusted dealer fallback)",
                cli.share_index
            );
        } else {
            println!(
                "Guardian {} has no keys — waiting for redeal/receive or DKG",
                cli.share_index
            );
        }
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    let app = Router::new()
        .route("/round1", post(round1_handler))
        .route("/round2", post(round2_handler))
        .route("/dkg/round1", post(dkg_round1_handler))
        .route("/dkg/round2", post(dkg_round2_handler))
        .route("/refresh/round1", post(refresh_round1_handler))
        .route("/refresh/round2", post(refresh_round2_handler))
        .route("/redeal/share", post(redeal_share_handler))
        .route("/redeal/receive", post(redeal_receive_handler))
        .route("/public-key", get(public_key_handler))
        .route("/config", get(config_handler))
        .route("/health", get(health_handler))
        .layer(cors)
        .with_state(state.clone());

    let addr = format!("0.0.0.0:{}", port);
    println!("Guardian {} listening on {}", cli.share_index, addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    // Spawn DKG / refresh / redeal task as appropriate
    if cli.refresh {
        // Proactive refresh: requires keys on disk + --peers
        assert!(
            state.is_ready(),
            "--refresh requires keys on disk (run DKG first)"
        );
        assert!(
            !cli.peers.is_empty(),
            "--refresh requires --peers"
        );
        let refresh_state = state.clone();
        let peers = cli.peers.clone();
        tokio::spawn(async move {
            run_refresh(refresh_state, peers).await;
        });
    } else if cli.redeal {
        // Re-deal: coordinator collects shares, reconstructs, and re-splits
        assert!(
            state.is_ready(),
            "--redeal coordinator requires keys on disk"
        );
        let new_max = cli
            .new_max_signers
            .expect("--redeal requires --new-max-signers");
        let new_min = cli
            .new_min_signers
            .expect("--redeal requires --new-min-signers");
        let redeal_state = state.clone();
        let old_peers = cli.old_peers.clone();
        let new_peers = cli.peers.clone();
        tokio::spawn(async move {
            run_redeal(redeal_state, old_peers, new_peers, new_max, new_min).await;
        });
    } else if !cli.peers.is_empty() && !state.is_ready() {
        // DKG: no keys on disk, peers provided
        let dkg_state = state.clone();
        let peers = cli.peers.clone();
        tokio::spawn(async move {
            run_dkg(dkg_state, peers).await;
        });
    }

    // Spawn node monitor if --node-url provided
    if let Some(node_url) = cli.node_url {
        let monitor_state = state.clone();
        tokio::spawn(async move {
            monitor_contracts(monitor_state, node_url).await;
        });
    }

    axum::serve(listener, app).await.expect("Server failed");
}

// ─── Hex helpers ─────────────────────────────────────────────────────────────

mod hex {
    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        if s.len() % 2 != 0 {
            return Err("Odd-length hex string".to_string());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&s[i..i + 2], 16)
                    .map_err(|e| format!("Invalid hex at position {}: {}", i, e))
            })
            .collect()
    }
}
