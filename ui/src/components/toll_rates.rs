use cream_common::tolls::TollRates;
use dioxus::prelude::*;

#[derive(Clone, Copy, Default, PartialEq)]
pub struct AdminStatus {
    pub admin: bool,
    pub root: bool,
}

/// Get the toll rates signal from context.
#[allow(dead_code)]
pub fn use_toll_rates() -> Signal<TollRates> {
    use_context::<Signal<TollRates>>()
}

/// Check whether a user pubkey is an admin on the guardian.
/// Returns default (not admin, not root) on error (conservative).
#[allow(dead_code)]
pub async fn check_admin_status(pubkey_hex: &str) -> AdminStatus {
    #[cfg(target_family = "wasm")]
    {
        let urls = super::signing_service::guardian_urls();
        if urls.is_empty() {
            return AdminStatus::default();
        }
        let url = format!("{}/admin-check?pubkey={}", urls[0], pubkey_hex);
        match fetch_json(&url).await {
            Ok(text) => {
                #[derive(serde::Deserialize)]
                struct Resp {
                    admin: bool,
                    #[serde(default)]
                    root: bool,
                }
                serde_json::from_str::<Resp>(&text)
                    .map(|r| AdminStatus { admin: r.admin, root: r.root })
                    .unwrap_or_default()
            }
            Err(e) => {
                web_sys::console::log_1(
                    &format!("[CREAM] Admin check failed: {}", e).into(),
                );
                AdminStatus::default()
            }
        }
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = pubkey_hex;
        AdminStatus::default()
    }
}

/// Fetch the list of all admin pubkeys (root only).
#[allow(dead_code)]
pub async fn fetch_admin_list(pubkey_hex: &str) -> Result<Vec<String>, String> {
    #[cfg(target_family = "wasm")]
    {
        let urls = super::signing_service::guardian_urls();
        if urls.is_empty() {
            return Err("No guardian URLs configured".to_string());
        }
        let url = format!("{}/admin-list?pubkey={}", urls[0], pubkey_hex);
        let text = fetch_json(&url).await?;
        #[derive(serde::Deserialize)]
        struct Resp {
            admins: Vec<String>,
        }
        serde_json::from_str::<Resp>(&text)
            .map(|r| r.admins)
            .map_err(|e| format!("Deserialize: {}", e))
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = pubkey_hex;
        Err("Not supported outside WASM".to_string())
    }
}

/// Grant admin status to a pubkey (root only).
#[allow(dead_code)]
pub async fn grant_admin(pubkey_hex: &str, grantor_hex: &str) -> Result<Vec<String>, String> {
    #[cfg(target_family = "wasm")]
    {
        let urls = super::signing_service::guardian_urls();
        if urls.is_empty() {
            return Err("No guardian URLs configured".to_string());
        }
        let url = format!("{}/admin-grant", urls[0]);
        let body = serde_json::json!({ "pubkey": pubkey_hex, "grantor": grantor_hex }).to_string();
        let text = post_json(&url, &body).await?;
        #[derive(serde::Deserialize)]
        struct Resp {
            admins: Vec<String>,
        }
        serde_json::from_str::<Resp>(&text)
            .map(|r| r.admins)
            .map_err(|e| format!("Deserialize: {}", e))
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = (pubkey_hex, grantor_hex);
        Err("Not supported outside WASM".to_string())
    }
}

/// Revoke admin status from a pubkey (root only).
#[allow(dead_code)]
pub async fn revoke_admin(pubkey_hex: &str, grantor_hex: &str) -> Result<Vec<String>, String> {
    #[cfg(target_family = "wasm")]
    {
        let urls = super::signing_service::guardian_urls();
        if urls.is_empty() {
            return Err("No guardian URLs configured".to_string());
        }
        let url = format!("{}/admin-revoke", urls[0]);
        let body = serde_json::json!({ "pubkey": pubkey_hex, "grantor": grantor_hex }).to_string();
        let text = post_json(&url, &body).await?;
        #[derive(serde::Deserialize)]
        struct Resp {
            admins: Vec<String>,
        }
        serde_json::from_str::<Resp>(&text)
            .map(|r| r.admins)
            .map_err(|e| format!("Deserialize: {}", e))
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = (pubkey_hex, grantor_hex);
        Err("Not supported outside WASM".to_string())
    }
}


#[cfg(target_family = "wasm")]
async fn fetch_json(url: &str) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(web_sys::RequestMode::Cors);

    let request = web_sys::Request::new_with_str_and_init(url, &opts)
        .map_err(|e| format!("Failed to create request: {:?}", e))?;

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

    text.as_string().ok_or("Response is not a string".into())
}

#[cfg(target_family = "wasm")]
async fn post_json(url: &str, body: &str) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_mode(web_sys::RequestMode::Cors);
    opts.set_body(&wasm_bindgen::JsValue::from_str(body));

    let request = web_sys::Request::new_with_str_and_init(url, &opts)
        .map_err(|e| format!("Failed to create request: {:?}", e))?;
    request
        .headers()
        .set("Content-Type", "application/json")
        .map_err(|e| format!("Failed to set header: {:?}", e))?;

    let window = web_sys::window().ok_or("No window")?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("Fetch failed: {:?}", e))?;

    let resp: web_sys::Response = resp_value
        .dyn_into()
        .map_err(|_| "Response is not a Response object".to_string())?;

    if !resp.ok() {
        let status = resp.status();
        let err_text = JsFuture::from(resp.text().map_err(|_| "Failed to get error text".to_string())?)
            .await
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, err_text));
    }

    let text = JsFuture::from(
        resp.text()
            .map_err(|e| format!("Failed to get text: {:?}", e))?,
    )
    .await
    .map_err(|e| format!("Failed to read body: {:?}", e))?;

    text.as_string().ok_or("Response is not a string".into())
}
