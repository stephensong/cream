//! FROST threshold signing service.
//!
//! Provides two modes:
//! - **Local**: all shares in-process (wraps `root_sign()` — current trusted-dealer behavior)
//! - **Remote**: coordinates with guardian daemons over HTTP for distributed signing

use serde::{Deserialize, Serialize};

/// Compile-time guardian URLs, overridable via `CREAM_GUARDIAN_URLS` env var.
/// Comma-separated list of URLs, e.g. "http://localhost:3010,http://localhost:3011,http://localhost:3012"
#[allow(dead_code)] // used in WASM builds
fn guardian_urls() -> Vec<String> {
    option_env!("CREAM_GUARDIAN_URLS")
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Signing service for FROST threshold signatures.
#[allow(dead_code)] // used in WASM builds
#[derive(Clone)]
pub enum SigningService {
    /// All shares in-process (trusted dealer mode).
    Local,
    /// Distributed signing via guardian HTTP daemons.
    Remote {
        guardian_urls: Vec<String>,
        /// Cached public key package, fetched from guardian on first use.
        cached_pubkey: std::sync::Arc<std::sync::Mutex<Option<frost_ed25519::keys::PublicKeyPackage>>>,
    },
}

#[allow(dead_code)] // used in WASM builds
impl SigningService {
    /// Create a signing service based on compile-time configuration.
    /// If `CREAM_GUARDIAN_URLS` is set, uses remote guardians; otherwise local.
    pub fn from_env() -> Self {
        let urls = guardian_urls();
        if urls.is_empty() {
            SigningService::Local
        } else {
            SigningService::Remote {
                guardian_urls: urls,
                cached_pubkey: std::sync::Arc::new(std::sync::Mutex::new(None)),
            }
        }
    }

    /// Sign a message using FROST threshold signatures.
    pub async fn sign(&self, message: &[u8]) -> Result<ed25519_dalek::Signature, String> {
        match self {
            SigningService::Local => Ok(cream_common::identity::root_sign(message)),
            #[cfg(target_family = "wasm")]
            SigningService::Remote { guardian_urls, cached_pubkey } => {
                wasm_impl::remote_sign(guardian_urls, message, cached_pubkey).await
            }
            #[cfg(not(target_family = "wasm"))]
            SigningService::Remote { .. } => {
                Err("Remote signing only available in WASM".into())
            }
        }
    }
}

// ─── API types (shared with guardian daemon) ─────────────────────────────────

#[derive(Serialize, Deserialize)]
struct Round1Request {
    session_id: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Round1Response {
    identifier: frost_ed25519::Identifier,
    commitments: frost_ed25519::round1::SigningCommitments,
}

#[derive(Serialize)]
struct Round2Request {
    session_id: String,
    message_hex: String,
    signing_commitments: Vec<Round1Response>,
}

#[derive(Deserialize)]
struct Round2Response {
    identifier: frost_ed25519::Identifier,
    signature_share: frost_ed25519::round2::SignatureShare,
}

// ─── WASM implementation ─────────────────────────────────────────────────────

#[cfg(target_family = "wasm")]
mod wasm_impl {
    use super::*;
    use frost_ed25519 as frost;
    use std::collections::BTreeMap;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    /// Minimum signers for the 2-of-3 threshold.
    const MIN_SIGNERS: usize = 2;

    /// Perform distributed FROST signing via guardian HTTP daemons.
    pub async fn remote_sign(
        guardian_urls: &[String],
        message: &[u8],
        cached_pubkey: &std::sync::Arc<std::sync::Mutex<Option<frost::keys::PublicKeyPackage>>>,
    ) -> Result<ed25519_dalek::Signature, String> {
        let session_id = generate_session_id()?;
        let message_hex = bytes_to_hex(message);

        // Round 1: collect commitments from all guardians concurrently
        let round1_futures: Vec<_> = guardian_urls
            .iter()
            .map(|url| {
                let url = url.clone();
                let session_id = session_id.clone();
                async move {
                    let req = Round1Request {
                        session_id: session_id.clone(),
                    };
                    let body = serde_json::to_string(&req).map_err(|e| e.to_string())?;
                    let resp_text =
                        fetch_json(&format!("{}/round1", url), "POST", Some(body)).await?;
                    let resp: Round1Response =
                        serde_json::from_str(&resp_text).map_err(|e| {
                            format!("Failed to parse round1 response from {}: {}", url, e)
                        })?;
                    Ok::<(String, Round1Response), String>((url, resp))
                }
            })
            .collect();

        // Await all and take the first MIN_SIGNERS successes
        let results = futures::future::join_all(round1_futures).await;
        let mut successes: Vec<(String, Round1Response)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for result in results {
            match result {
                Ok(s) => successes.push(s),
                Err(e) => errors.push(e),
            }
        }

        if successes.len() < MIN_SIGNERS {
            return Err(format!(
                "Only {} of {} guardians responded to round1 (need {}). Errors: {}",
                successes.len(),
                guardian_urls.len(),
                MIN_SIGNERS,
                errors.join("; ")
            ));
        }

        // Take exactly MIN_SIGNERS participants
        let participants: Vec<(String, Round1Response)> =
            successes.into_iter().take(MIN_SIGNERS).collect();
        let all_commitments: Vec<Round1Response> =
            participants.iter().map(|(_, r)| r.clone()).collect();

        // Round 2: send commitments map + message to each participant
        let round2_futures: Vec<_> = participants
            .iter()
            .map(|(url, _)| {
                let url = url.clone();
                let session_id = session_id.clone();
                let message_hex = message_hex.clone();
                let signing_commitments = all_commitments.clone();
                async move {
                    let req = Round2Request {
                        session_id,
                        message_hex,
                        signing_commitments,
                    };
                    let body = serde_json::to_string(&req).map_err(|e| e.to_string())?;
                    let resp_text =
                        fetch_json(&format!("{}/round2", url), "POST", Some(body)).await?;
                    let resp: Round2Response =
                        serde_json::from_str(&resp_text).map_err(|e| {
                            format!("Failed to parse round2 response from {}: {}", url, e)
                        })?;
                    Ok::<Round2Response, String>(resp)
                }
            })
            .collect();

        let round2_results = futures::future::join_all(round2_futures).await;
        let mut signature_shares: BTreeMap<frost::Identifier, frost::round2::SignatureShare> =
            BTreeMap::new();
        let mut round2_errors: Vec<String> = Vec::new();

        for result in round2_results {
            match result {
                Ok(resp) => {
                    signature_shares.insert(resp.identifier, resp.signature_share);
                }
                Err(e) => round2_errors.push(e),
            }
        }

        if signature_shares.len() < MIN_SIGNERS {
            return Err(format!(
                "Only {} of {} guardians completed round2 (need {}). Errors: {}",
                signature_shares.len(),
                MIN_SIGNERS,
                MIN_SIGNERS,
                round2_errors.join("; ")
            ));
        }

        // Build commitments map for aggregation
        let commitments_map: BTreeMap<frost::Identifier, frost::round1::SigningCommitments> =
            all_commitments
                .into_iter()
                .map(|c| (c.identifier, c.commitments))
                .collect();

        // Aggregate signature shares
        let signing_package = frost::SigningPackage::new(commitments_map, message);

        // Fetch public key from guardian (cached after first call)
        let pubkey_package = {
            let existing = cached_pubkey.lock().unwrap().clone();
            if let Some(pkg) = existing {
                pkg
            } else {
                let pkg = fetch_public_key(&guardian_urls[0]).await?;
                *cached_pubkey.lock().unwrap() = Some(pkg.clone());
                pkg
            }
        };

        let group_signature =
            frost::aggregate(&signing_package, &signature_shares, &pubkey_package)
                .map_err(|e| format!("FROST aggregation failed: {}", e))?;

        // Convert frost::Signature → ed25519_dalek::Signature
        let sig_vec = group_signature
            .serialize()
            .expect("signature serialization should not fail");
        let sig_bytes: [u8; 64] = sig_vec
            .try_into()
            .expect("signature is 64 bytes");
        Ok(ed25519_dalek::Signature::from_bytes(&sig_bytes))
    }

    /// Fetch the PublicKeyPackage from a guardian's /public-key endpoint.
    async fn fetch_public_key(
        guardian_url: &str,
    ) -> Result<frost::keys::PublicKeyPackage, String> {
        let resp_text =
            fetch_json(&format!("{}/public-key", guardian_url), "GET", None).await?;
        serde_json::from_str(&resp_text).map_err(|e| {
            format!(
                "Failed to parse public key from {}: {}",
                guardian_url, e
            )
        })
    }

    /// Fetch JSON via web_sys (same pattern as rendezvous.rs).
    async fn fetch_json(
        url: &str,
        method: &str,
        body: Option<String>,
    ) -> Result<String, String> {
        let opts = web_sys::RequestInit::new();
        opts.set_method(method);
        opts.set_mode(web_sys::RequestMode::Cors);

        if let Some(b) = body {
            opts.set_body(&wasm_bindgen::JsValue::from_str(&b));
        }

        let request = web_sys::Request::new_with_str_and_init(url, &opts)
            .map_err(|e| format!("Failed to create request: {:?}", e))?;

        if method == "POST" {
            request
                .headers()
                .set("Content-Type", "application/json")
                .map_err(|e| format!("Failed to set header: {:?}", e))?;
        }

        let window = web_sys::window().ok_or("No window")?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Fetch failed: {:?}", e))?;

        let resp: web_sys::Response = resp_value
            .dyn_into()
            .map_err(|_| "Response is not a Response object".to_string())?;

        let text = JsFuture::from(
            resp.text()
                .map_err(|e| format!("Failed to get text: {:?}", e))?,
        )
        .await
        .map_err(|e| format!("Failed to read body: {:?}", e))?;

        let text_str = text
            .as_string()
            .ok_or("Response body is not a string".to_string())?;

        let status = resp.status();
        if status >= 400 {
            return Err(format!("HTTP {} from {}: {}", status, url, text_str));
        }

        Ok(text_str)
    }

    /// Generate a random session ID (hex-encoded 16 bytes).
    fn generate_session_id() -> Result<String, String> {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes).map_err(|e| format!("RNG error: {}", e))?;
        Ok(bytes_to_hex(&bytes))
    }

    /// Encode bytes as lowercase hex string.
    fn bytes_to_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
