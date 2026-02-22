use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(target_family = "wasm")]
const STORAGE_KEY: &str = "cream_user_state";
#[cfg(target_family = "wasm")]
const PASSWORD_KEY: &str = "cream_password";

/// A placed order tracked in the UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedOrder {
    pub id: u32,
    pub supplier: String,
    pub product: String,
    pub quantity: u32,
    pub deposit_tier: String,
    pub price_per_unit: u64,
    pub status: String,
}

/// A product listed by the current user (as supplier).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListedProduct {
    pub id: u32,
    pub name: String,
    pub category: String,
    pub description: String,
    pub price_curd: u64,
    pub quantity_total: u32,
}

fn default_balance() -> u64 {
    10_000
}

/// Shared application state accessible from all components.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserState {
    pub moniker: Option<String>,
    pub postcode: Option<String>,
    #[serde(default)]
    pub locality: Option<String>,
    pub is_supplier: bool,
    pub supplier_description: Option<String>,
    pub products: Vec<ListedProduct>,
    pub orders: Vec<PlacedOrder>,
    pub next_order_id: u32,
    pub next_product_id: u32,
    /// Mock CURD wallet balance. Starts at 10,000, decremented by order deposits.
    #[serde(default = "default_balance")]
    pub balance: u64,
}

impl UserState {
    pub fn new() -> Self {
        if let Some(state) = Self::load() {
            return state;
        }
        Self {
            moniker: None,
            postcode: None,
            locality: None,
            is_supplier: false,
            supplier_description: None,
            products: Vec::new(),
            orders: Vec::new(),
            next_order_id: 1,
            next_product_id: 1,
            balance: 10_000,
        }
    }

    /// Save current state to sessionStorage (per-tab, survives refresh).
    pub fn save(&self) {
        #[cfg(target_family = "wasm")]
        {
            if let Ok(json) = serde_json::to_string(self) {
                if let Some(storage) = web_sys::window()
                    .and_then(|w| w.session_storage().ok())
                    .flatten()
                {
                    let _ = storage.set_item(STORAGE_KEY, &json);
                }
            }
        }
    }

    /// Load state from sessionStorage.
    fn load() -> Option<Self> {
        #[cfg(target_family = "wasm")]
        {
            let storage = web_sys::window()?.session_storage().ok()??;
            let json = storage.get_item(STORAGE_KEY).ok()??;
            serde_json::from_str(&json).ok()
        }
        #[cfg(not(target_family = "wasm"))]
        {
            None
        }
    }

    /// Save password to sessionStorage (per-tab).
    pub fn save_password(password: &str) {
        #[cfg(target_family = "wasm")]
        {
            if let Some(storage) = web_sys::window()
                .and_then(|w| w.session_storage().ok())
                .flatten()
            {
                let _ = storage.set_item(PASSWORD_KEY, password);
            }
        }
        let _ = password;
    }

    /// Load password from sessionStorage.
    pub fn load_password() -> Option<String> {
        #[cfg(target_family = "wasm")]
        {
            let storage = web_sys::window()?.session_storage().ok()??;
            storage.get_item(PASSWORD_KEY).ok()?
        }
        #[cfg(not(target_family = "wasm"))]
        {
            None
        }
    }

    /// Clear all session data (for "log out" / "use different identity").
    pub fn clear_session() {
        #[cfg(target_family = "wasm")]
        {
            if let Some(storage) = web_sys::window()
                .and_then(|w| w.session_storage().ok())
                .flatten()
            {
                let _ = storage.remove_item(STORAGE_KEY);
                let _ = storage.remove_item(PASSWORD_KEY);
            }
        }
    }

    pub fn place_order(
        &mut self,
        supplier: String,
        product: String,
        quantity: u32,
        deposit_tier: String,
        price_per_unit: u64,
    ) -> u32 {
        let id = self.next_order_id;
        self.next_order_id += 1;

        // Deduct deposit from wallet balance
        let total = price_per_unit * quantity as u64;
        let deposit = match deposit_tier.as_str() {
            "2-Day Reserve (10%)" => total / 10,
            "1-Week Reserve (20%)" => total / 5,
            _ => total, // Full Payment
        };
        self.balance = self.balance.saturating_sub(deposit);

        self.orders.push(PlacedOrder {
            id,
            supplier,
            product,
            quantity,
            deposit_tier,
            price_per_unit,
            status: "Reserved".into(),
        });
        self.save();
        id
    }

    pub fn add_product(
        &mut self,
        name: String,
        category: String,
        description: String,
        price_curd: u64,
        quantity_total: u32,
    ) -> u32 {
        let id = self.next_product_id;
        self.next_product_id += 1;
        self.products.push(ListedProduct {
            id,
            name,
            category,
            description,
            price_curd,
            quantity_total,
        });
        self.save();
        id
    }

    #[allow(dead_code)] // TODO: wire up to UI
    pub fn remove_product(&mut self, id: u32) {
        self.products.retain(|p| p.id != id);
        self.save();
    }
}

/// Provide UserState as shared context at the top of the app.
pub fn use_user_state() -> Signal<UserState> {
    use_context::<Signal<UserState>>()
}
