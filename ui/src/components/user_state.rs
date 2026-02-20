use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(target_family = "wasm")]
const STORAGE_KEY: &str = "cream_user_state";

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
    pub quantity_available: u32,
}

/// Shared application state accessible from all components.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserState {
    pub moniker: Option<String>,
    pub postcode: Option<String>,
    pub is_supplier: bool,
    pub supplier_description: Option<String>,
    pub products: Vec<ListedProduct>,
    pub orders: Vec<PlacedOrder>,
    pub next_order_id: u32,
    pub next_product_id: u32,
}

impl UserState {
    pub fn new() -> Self {
        if let Some(state) = Self::load() {
            return state;
        }
        Self {
            moniker: None,
            postcode: None,
            is_supplier: false,
            supplier_description: None,
            products: Vec::new(),
            orders: Vec::new(),
            next_order_id: 1,
            next_product_id: 1,
        }
    }

    /// Save current state to localStorage.
    pub fn save(&self) {
        #[cfg(target_family = "wasm")]
        {
            if let Ok(json) = serde_json::to_string(self) {
                if let Some(storage) = web_sys::window()
                    .and_then(|w| w.local_storage().ok())
                    .flatten()
                {
                    let _ = storage.set_item(STORAGE_KEY, &json);
                }
            }
        }
    }

    /// Load state from localStorage.
    fn load() -> Option<Self> {
        #[cfg(target_family = "wasm")]
        {
            let storage = web_sys::window()?.local_storage().ok()??;
            let json = storage.get_item(STORAGE_KEY).ok()??;
            serde_json::from_str(&json).ok()
        }
        #[cfg(not(target_family = "wasm"))]
        {
            None
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
        quantity_available: u32,
    ) -> u32 {
        let id = self.next_product_id;
        self.next_product_id += 1;
        self.products.push(ListedProduct {
            id,
            name,
            category,
            description,
            price_curd,
            quantity_available,
        });
        self.save();
        id
    }

    pub fn remove_product(&mut self, id: u32) {
        self.products.retain(|p| p.id != id);
        self.save();
    }
}

/// Provide UserState as shared context at the top of the app.
pub fn use_user_state() -> Signal<UserState> {
    use_context::<Signal<UserState>>()
}
