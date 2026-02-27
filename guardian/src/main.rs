//! CREAM FROST guardian daemon.
//!
//! Holds one FROST key share and participates in distributed threshold signing
//! via HTTP. In dev mode, key shares are derived deterministically from
//! `dev_root_frost_keys()`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::Method;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

/// TTL for stored nonces (seconds). Expired nonces are cleaned on each round1 call.
const NONCE_TTL_SECS: u64 = 30;

#[derive(Parser)]
#[command(name = "cream-guardian", about = "CREAM FROST guardian daemon")]
struct Cli {
    /// Guardian share index (1, 2, or 3 for the default 2-of-3 setup).
    #[arg(long, default_value_t = 1)]
    share_index: u16,

    /// HTTP port to listen on (default: 3009 + share_index).
    #[arg(long)]
    port: Option<u16>,
}

struct AppState {
    key_package: frost::keys::KeyPackage,
    identifier: frost::Identifier,
    nonces: Mutex<BTreeMap<String, (frost::round1::SigningNonces, Instant)>>,
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
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn round1_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<Round1Request>,
) -> Result<Json<Round1Response>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    use rand::rngs::OsRng;

    let (nonces, commitments) =
        frost::round1::commit(state.key_package.signing_share(), &mut OsRng);

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
        frost::round2::sign(&signing_package, &nonces, &state.key_package).map_err(|e| {
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

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        identifier: state.identifier,
    })
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let identifier = frost::Identifier::try_from(cli.share_index)
        .expect("Invalid share_index (must be 1..=max_signers)");

    let (key_packages, _pubkey_package) = cream_common::frost::dev_root_frost_keys();

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

    let port = cli.port.unwrap_or(3009 + cli.share_index);

    let state = Arc::new(AppState {
        key_package,
        identifier,
        nonces: Mutex::new(BTreeMap::new()),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    let app = Router::new()
        .route("/round1", post(round1_handler))
        .route("/round2", post(round2_handler))
        .route("/health", get(health_handler))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!(
        "Guardian {} listening on {}",
        cli.share_index, addr
    );

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");
    axum::serve(listener, app).await.expect("Server failed");
}

// ─── Hex helpers (avoid adding a `hex` crate dependency) ─────────────────────

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
