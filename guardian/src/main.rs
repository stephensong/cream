//! CREAM FROST guardian daemon.
//!
//! Holds one FROST key share and participates in distributed threshold signing
//! via HTTP. Supports three startup modes:
//!
//! 1. **Keys on disk** → load and activate immediately
//! 2. **`--peers` provided, no keys** → run DKG ceremony with peer guardians
//! 3. **No `--peers`, no keys** → `dev_root_frost_keys()` fallback (trusted dealer)

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::Method;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::{Any, CorsLayer};

/// TTL for stored nonces (seconds). Expired nonces are cleaned on each round1 call.
const NONCE_TTL_SECS: u64 = 30;

/// Max signers for the default 2-of-3 setup.
const MAX_SIGNERS: u16 = 3;
/// Min signers (threshold) for the default 2-of-3 setup.
const MIN_SIGNERS: u16 = 2;

#[derive(Parser)]
#[command(name = "cream-guardian", about = "CREAM FROST guardian daemon")]
struct Cli {
    /// Guardian share index (1, 2, or 3 for the default 2-of-3 setup).
    #[arg(long, default_value_t = 1)]
    share_index: u16,

    /// HTTP port to listen on (default: 3009 + share_index).
    #[arg(long)]
    port: Option<u16>,

    /// Comma-separated peer guardian URLs for DKG
    /// (e.g. "http://localhost:3011,http://localhost:3012").
    #[arg(long, value_delimiter = ',')]
    peers: Vec<String>,
}

struct AppState {
    identifier: frost::Identifier,
    share_index: u16,
    key_package: RwLock<Option<frost::keys::KeyPackage>>,
    public_key_package: RwLock<Option<frost::keys::PublicKeyPackage>>,
    nonces: Mutex<BTreeMap<String, (frost::round1::SigningNonces, Instant)>>,
    dkg_state: Mutex<DkgState>,
}

impl AppState {
    fn is_ready(&self) -> bool {
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
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
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

    let n_peers = peers.len();
    println!(
        "DKG: starting ceremony with {} peers (total {} guardians)",
        n_peers,
        n_peers + 1
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
        MAX_SIGNERS,
        MIN_SIGNERS,
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
        let peer_url = identifier_to_peer_url(*recipient_id, state.share_index, &peers);
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

/// Map a FROST Identifier to its peer URL.
///
/// Peer URLs are ordered by their share index, excluding our own index.
/// FROST identifiers are 1-based u16: 1, 2, 3.
/// If our share_index is 2 and peers are ["http://...:3010", "http://...:3012"],
/// then peer indices are [1, 3] (all indices except 2).
fn identifier_to_peer_url(
    target: frost::Identifier,
    our_share_index: u16,
    peers: &[String],
) -> Option<String> {
    // Build the list of peer share indices (all valid indices except ours)
    let peer_indices: Vec<u16> = (1..=MAX_SIGNERS)
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
        key_package: RwLock::new(None),
        public_key_package: RwLock::new(None),
        nonces: Mutex::new(BTreeMap::new()),
        dkg_state: Mutex::new(DkgState::default()),
    });

    // ── Key initialization ──
    if let Some(persisted) = load_keys(cli.share_index) {
        // Mode 1: Keys on disk → activate immediately
        *state.key_package.write().await = Some(persisted.key_package);
        *state.public_key_package.write().await = Some(persisted.public_key_package);
        println!("Guardian {} ready (loaded from disk)", cli.share_index);
    } else if !cli.peers.is_empty() {
        // Mode 2: --peers provided → DKG will run after server starts
        println!(
            "Guardian {} will run DKG with {} peers",
            cli.share_index,
            cli.peers.len()
        );
    } else {
        // Mode 3: No peers, no keys → trusted dealer fallback
        let (key_packages, pubkey_package) = cream_common::frost::dev_root_frost_keys();
        let key_package = key_packages
            .get(&identifier)
            .unwrap_or_else(|| {
                panic!(
                    "No key package for share_index {}. Valid indices: {:?}",
                    cli.share_index,
                    key_packages.keys().collect::<Vec<_>>()
                )
            })
            .clone();
        *state.key_package.write().await = Some(key_package);
        *state.public_key_package.write().await = Some(pubkey_package);
        println!(
            "Guardian {} ready (trusted dealer fallback)",
            cli.share_index
        );
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
        .route("/public-key", get(public_key_handler))
        .route("/health", get(health_handler))
        .layer(cors)
        .with_state(state.clone());

    let addr = format!("0.0.0.0:{}", port);
    println!("Guardian {} listening on {}", cli.share_index, addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    // Spawn DKG task if needed (Mode 2)
    if !cli.peers.is_empty() && !state.is_ready() {
        let dkg_state = state.clone();
        let peers = cli.peers.clone();
        tokio::spawn(async move {
            run_dkg(dkg_state, peers).await;
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
