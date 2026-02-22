use serde::{Deserialize, Serialize};

/// Default rendezvous service URL, overridable at compile time.
#[allow(dead_code)] // used in WASM builds
const DEFAULT_RENDEZVOUS_URL: &str = "https://cream-rendezvous.workers.dev";

#[allow(dead_code)] // used in WASM builds
fn rendezvous_url() -> &'static str {
    option_env!("CREAM_RENDEZVOUS_URL").unwrap_or(DEFAULT_RENDEZVOUS_URL)
}

/// A supplier entry returned by the rendezvous service.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RendezvousEntry {
    pub name: String,
    pub address: String,
    pub storefront_key: String,
}

/// Error response from the rendezvous service.
#[allow(dead_code)] // used in WASM builds
#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: String,
}

// ─── WASM implementation ─────────────────────────────────────────────────────

#[cfg(target_family = "wasm")]
mod wasm_impl {
    use super::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    async fn fetch_json(url: &str, method: &str, body: Option<String>) -> Result<String, String> {
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
            resp.text().map_err(|e| format!("Failed to get text: {:?}", e))?,
        )
        .await
        .map_err(|e| format!("Failed to read body: {:?}", e))?;

        let text_str = text
            .as_string()
            .ok_or("Response body is not a string".to_string())?;

        let status = resp.status();
        if status >= 400 {
            if let Ok(err) = serde_json::from_str::<ErrorBody>(&text_str) {
                return Err(err.error);
            }
            return Err(format!("HTTP {}: {}", status, text_str));
        }

        Ok(text_str)
    }

    /// Look up a supplier by name.
    pub async fn lookup_supplier(name: &str) -> Result<RendezvousEntry, String> {
        let url = format!("{}/lookup/{}", rendezvous_url(), urlencoding(name));
        let text = fetch_json(&url, "GET", None).await?;
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse response: {}", e))
    }

    /// Register a supplier with the rendezvous service.
    pub async fn register_supplier(
        name: &str,
        address: &str,
        storefront_key: &str,
        public_key_hex: &str,
        signature_hex: &str,
    ) -> Result<(), String> {
        let url = format!("{}/register", rendezvous_url());
        let body = serde_json::json!({
            "name": name,
            "address": address,
            "storefront_key": storefront_key,
            "public_key": public_key_hex,
            "signature": signature_hex,
        });
        fetch_json(&url, "POST", Some(body.to_string())).await?;
        Ok(())
    }

    /// Send a heartbeat to refresh TTL and update address.
    pub async fn heartbeat(
        name: &str,
        address: &str,
        public_key_hex: &str,
        signature_hex: &str,
    ) -> Result<(), String> {
        let url = format!("{}/heartbeat", rendezvous_url());
        let body = serde_json::json!({
            "name": name,
            "address": address,
            "public_key": public_key_hex,
            "signature": signature_hex,
        });
        fetch_json(&url, "POST", Some(body.to_string())).await?;
        Ok(())
    }

    /// Simple percent-encoding for URL path segments.
    fn urlencoding(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                    c.to_string()
                }
                _ => format!("%{:02X}", c as u32),
            })
            .collect()
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Look up a supplier by name from the rendezvous service.
#[allow(dead_code)] // used in customer builds
pub async fn lookup_supplier(name: &str) -> Result<RendezvousEntry, String> {
    #[cfg(target_family = "wasm")]
    {
        wasm_impl::lookup_supplier(name).await
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = name;
        Err("Rendezvous client only available in WASM".into())
    }
}

/// Register a supplier with the rendezvous service.
#[allow(dead_code)] // used in supplier builds
pub async fn register_supplier(
    name: &str,
    address: &str,
    storefront_key: &str,
    public_key_hex: &str,
    signature_hex: &str,
) -> Result<(), String> {
    #[cfg(target_family = "wasm")]
    {
        wasm_impl::register_supplier(name, address, storefront_key, public_key_hex, signature_hex)
            .await
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = (name, address, storefront_key, public_key_hex, signature_hex);
        Err("Rendezvous client only available in WASM".into())
    }
}

/// Send a heartbeat to the rendezvous service.
#[allow(dead_code)] // used in supplier builds
pub async fn heartbeat(
    name: &str,
    address: &str,
    public_key_hex: &str,
    signature_hex: &str,
) -> Result<(), String> {
    #[cfg(target_family = "wasm")]
    {
        wasm_impl::heartbeat(name, address, public_key_hex, signature_hex).await
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = (name, address, public_key_hex, signature_hex);
        Err("Rendezvous client only available in WASM".into())
    }
}
