//! WASM HTTP client for the Lightning gateway guardian.
//!
//! Calls the `/lightning/*` endpoints on the designated gateway guardian.
//! Gateway URL is determined from `CREAM_GATEWAY_URL` or the first entry
//! in `CREAM_GUARDIAN_URLS`.

use serde::{Deserialize, Serialize};

/// Get the gateway guardian URL from compile-time env vars.
#[allow(dead_code)]
fn gateway_url() -> Option<String> {
    // Prefer explicit gateway URL, fall back to first guardian URL
    if let Some(url) = option_env!("CREAM_GATEWAY_URL") {
        if !url.is_empty() {
            return Some(url.to_string());
        }
    }
    option_env!("CREAM_GUARDIAN_URLS")
        .and_then(|urls| urls.split(',').next())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

// ─── Request/Response types ──────────────────────────────────────────────────

#[derive(Serialize)]
struct PegInRequest {
    amount_sats: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PegInResponse {
    pub bolt11: String,
    pub payment_hash: String,
    pub amount_sats: u64,
}

#[derive(Serialize)]
struct PegInCheckRequest {
    payment_hash: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PegInCheckResponse {
    pub status: String,
    pub payment_hash: String,
}

#[derive(Serialize)]
struct PegInSettleRequest {
    payment_hash: String,
}

#[derive(Serialize)]
struct PegInCancelRequest {
    payment_hash: String,
}

#[derive(Serialize)]
struct PegOutRequest {
    bolt11: String,
    amount_sats: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PegOutResponse {
    pub success: bool,
    pub preimage: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LndInfo {
    pub pubkey: String,
    pub alias: String,
    pub synced_to_chain: bool,
    pub synced_to_graph: bool,
    pub block_height: u32,
    pub num_active_channels: u32,
    pub num_peers: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BalanceResponse {
    pub wallet: WalletBalance,
    pub channel: ChannelBalance,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WalletBalance {
    pub total_balance: i64,
    pub confirmed_balance: i64,
    pub unconfirmed_balance: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChannelBalance {
    pub local_balance_sat: u64,
    pub remote_balance_sat: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChannelInfo {
    pub channel_point: String,
    pub remote_pubkey: String,
    pub capacity: i64,
    pub local_balance: i64,
    pub remote_balance: i64,
    pub active: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PegTransaction {
    pub kind: String,
    pub payment_hash: String,
    pub amount_sats: u64,
    pub status: String,
    pub timestamp: u64,
}

#[derive(Serialize)]
struct OpenChannelRequest {
    node_pubkey: String,
    local_funding_amount: i64,
}

#[derive(Serialize)]
struct CloseChannelRequest {
    channel_point: String,
    force: bool,
}

// ─── Client ──────────────────────────────────────────────────────────────────

/// Lightning gateway client. Only functional in WASM builds with gateway URL configured.
#[allow(dead_code)]
pub struct LightningClient {
    base_url: String,
}

#[allow(dead_code)]
impl LightningClient {
    /// Create a new client from compile-time env vars. Returns None if no gateway configured.
    pub fn from_env() -> Option<Self> {
        gateway_url().map(|url| Self { base_url: url })
    }

    /// Check if a Lightning gateway is configured.
    pub fn is_available() -> bool {
        gateway_url().is_some()
    }

    /// Create a hold invoice for peg-in.
    pub async fn create_pegin_invoice(
        &self,
        amount_sats: u64,
    ) -> Result<PegInResponse, String> {
        let body = serde_json::to_string(&PegInRequest { amount_sats })
            .map_err(|e| e.to_string())?;
        let resp = post_json(&self.base_url, "/lightning/peg-in", &body).await?;
        serde_json::from_str(&resp).map_err(|e| format!("Parse peg-in response: {}", e))
    }

    /// Check status of a pending peg-in invoice.
    pub async fn check_pegin_status(
        &self,
        payment_hash: &str,
    ) -> Result<PegInCheckResponse, String> {
        let body = serde_json::to_string(&PegInCheckRequest {
            payment_hash: payment_hash.to_string(),
        })
        .map_err(|e| e.to_string())?;
        let resp = post_json(&self.base_url, "/lightning/peg-in/check", &body).await?;
        serde_json::from_str(&resp).map_err(|e| format!("Parse check response: {}", e))
    }

    /// Settle a hold invoice after CURD allocation.
    pub async fn settle_pegin(&self, payment_hash: &str) -> Result<(), String> {
        let body = serde_json::to_string(&PegInSettleRequest {
            payment_hash: payment_hash.to_string(),
        })
        .map_err(|e| e.to_string())?;
        post_json(&self.base_url, "/lightning/peg-in/settle", &body).await?;
        Ok(())
    }

    /// Cancel a hold invoice on failure.
    pub async fn cancel_pegin(&self, payment_hash: &str) -> Result<(), String> {
        let body = serde_json::to_string(&PegInCancelRequest {
            payment_hash: payment_hash.to_string(),
        })
        .map_err(|e| e.to_string())?;
        post_json(&self.base_url, "/lightning/peg-in/cancel", &body).await?;
        Ok(())
    }

    /// Pay a BOLT11 invoice for peg-out.
    pub async fn pay_invoice(
        &self,
        bolt11: &str,
        amount_sats: u64,
    ) -> Result<PegOutResponse, String> {
        let body = serde_json::to_string(&PegOutRequest {
            bolt11: bolt11.to_string(),
            amount_sats,
        })
        .map_err(|e| e.to_string())?;
        let resp = post_json(&self.base_url, "/lightning/peg-out", &body).await?;
        serde_json::from_str(&resp).map_err(|e| format!("Parse peg-out response: {}", e))
    }

    /// Get LND node info.
    pub async fn get_info(&self) -> Result<LndInfo, String> {
        let resp = get_json(&self.base_url, "/lightning/info").await?;
        serde_json::from_str(&resp).map_err(|e| format!("Parse info: {}", e))
    }

    /// Get on-chain + channel balances.
    pub async fn get_balance(&self) -> Result<BalanceResponse, String> {
        let resp = get_json(&self.base_url, "/lightning/balance").await?;
        serde_json::from_str(&resp).map_err(|e| format!("Parse balance: {}", e))
    }

    /// List channels.
    pub async fn list_channels(&self) -> Result<Vec<ChannelInfo>, String> {
        let resp = get_json(&self.base_url, "/lightning/channels").await?;
        serde_json::from_str(&resp).map_err(|e| format!("Parse channels: {}", e))
    }

    /// Get peg-in/peg-out history.
    pub async fn get_history(&self) -> Result<Vec<PegTransaction>, String> {
        let resp = get_json(&self.base_url, "/lightning/history").await?;
        serde_json::from_str(&resp).map_err(|e| format!("Parse history: {}", e))
    }

    /// Open a channel (operator action).
    pub async fn open_channel(
        &self,
        node_pubkey: &str,
        local_funding_amount: i64,
    ) -> Result<String, String> {
        let body = serde_json::to_string(&OpenChannelRequest {
            node_pubkey: node_pubkey.to_string(),
            local_funding_amount,
        })
        .map_err(|e| e.to_string())?;
        let resp = post_json(&self.base_url, "/lightning/channels/open", &body).await?;
        // Returns {"channel_point": "..."}
        #[derive(Deserialize)]
        struct Resp {
            channel_point: String,
        }
        let r: Resp =
            serde_json::from_str(&resp).map_err(|e| format!("Parse open channel: {}", e))?;
        Ok(r.channel_point)
    }

    /// Close a channel (operator action).
    pub async fn close_channel(
        &self,
        channel_point: &str,
        force: bool,
    ) -> Result<(), String> {
        let body = serde_json::to_string(&CloseChannelRequest {
            channel_point: channel_point.to_string(),
            force,
        })
        .map_err(|e| e.to_string())?;
        post_json(&self.base_url, "/lightning/channels/close", &body).await?;
        Ok(())
    }
}

// ─── HTTP helpers (WASM) ─────────────────────────────────────────────────────

#[cfg(target_family = "wasm")]
async fn post_json(base_url: &str, path: &str, body: &str) -> Result<String, String> {
    fetch_json(&format!("{}{}", base_url, path), "POST", Some(body.to_string())).await
}

#[cfg(target_family = "wasm")]
async fn get_json(base_url: &str, path: &str) -> Result<String, String> {
    fetch_json(&format!("{}{}", base_url, path), "GET", None).await
}

#[cfg(target_family = "wasm")]
async fn fetch_json(url: &str, method: &str, body: Option<String>) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

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

// Non-WASM stubs for type checking
#[cfg(not(target_family = "wasm"))]
async fn post_json(_base_url: &str, _path: &str, _body: &str) -> Result<String, String> {
    Err("Lightning client only available in WASM".to_string())
}

#[cfg(not(target_family = "wasm"))]
async fn get_json(_base_url: &str, _path: &str) -> Result<String, String> {
    Err("Lightning client only available in WASM".to_string())
}
