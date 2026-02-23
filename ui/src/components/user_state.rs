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

/// A single wallet transaction (credit or debit).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WalletTransaction {
    pub id: u32,
    pub kind: TransactionKind,
    pub amount: u64,
    pub description: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TransactionKind {
    Credit,
    Debit,
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
    /// Transaction ledger — balance is derived from sum of credits minus debits.
    #[serde(default)]
    pub ledger: Vec<WalletTransaction>,
    #[serde(default)]
    pub next_tx_id: u32,
    /// Legacy field for migration only. New code uses `balance()` method.
    #[serde(default)]
    balance: u64,
    /// Customer mode: connected supplier's memorable name (e.g. "garys-farm").
    #[serde(default)]
    pub connected_supplier: Option<String>,
    /// Customer mode: resolved WebSocket URL for the supplier's node.
    #[serde(default)]
    pub supplier_node_url: Option<String>,
    /// Customer mode: Base58-encoded ContractInstanceId for the supplier's storefront.
    #[serde(default)]
    pub supplier_storefront_key: Option<String>,
    /// Base58-encoded ContractKey for this user's own user contract.
    #[serde(default)]
    pub user_contract_key: Option<String>,
}

/// Get current time as ISO 8601 string.
fn now_iso8601() -> String {
    #[cfg(target_family = "wasm")]
    {
        web_sys::js_sys::Date::new_0().to_iso_string().into()
    }
    #[cfg(not(target_family = "wasm"))]
    {
        String::from("1970-01-01T00:00:00.000Z")
    }
}

impl UserState {
    pub fn new() -> Self {
        if let Some(mut state) = Self::load() {
            // Migrate: old sessionStorage had balance field but no ledger
            if state.ledger.is_empty() && state.balance > 0 {
                state.next_tx_id = 1;
                state.ledger.push(WalletTransaction {
                    id: 0,
                    kind: TransactionKind::Credit,
                    amount: state.balance,
                    description: "Migrated balance".into(),
                    timestamp: now_iso8601(),
                });
                state.balance = 0;
                state.save();
            }
            return state;
        }
        let mut state = Self {
            moniker: None,
            postcode: None,
            locality: None,
            is_supplier: false,
            supplier_description: None,
            products: Vec::new(),
            orders: Vec::new(),
            next_order_id: 1,
            next_product_id: 1,
            ledger: Vec::new(),
            next_tx_id: 0,
            balance: 0,
            connected_supplier: None,
            supplier_node_url: None,
            supplier_storefront_key: None,
            user_contract_key: None,
        };
        state.record_credit(10_000, "Initial CURD allocation".into());
        state
    }

    /// Derive balance from the transaction ledger.
    pub fn balance(&self) -> u64 {
        self.ledger.iter().fold(0u64, |acc, tx| match tx.kind {
            TransactionKind::Credit => acc.saturating_add(tx.amount),
            TransactionKind::Debit => acc.saturating_sub(tx.amount),
        })
    }

    /// Record a credit (faucet, refund, initial allocation). Returns the transaction ID.
    pub fn record_credit(&mut self, amount: u64, description: String) -> u32 {
        let id = self.next_tx_id;
        self.next_tx_id += 1;
        self.ledger.push(WalletTransaction {
            id,
            kind: TransactionKind::Credit,
            amount,
            description,
            timestamp: now_iso8601(),
        });
        id
    }

    /// Record a debit (order deposit). Returns `None` if insufficient funds.
    pub fn record_debit(&mut self, amount: u64, description: String) -> Option<u32> {
        if self.balance() < amount {
            return None;
        }
        let id = self.next_tx_id;
        self.next_tx_id += 1;
        self.ledger.push(WalletTransaction {
            id,
            kind: TransactionKind::Debit,
            amount,
            description,
            timestamp: now_iso8601(),
        });
        Some(id)
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
    ) -> Option<u32> {
        // Calculate deposit required
        let total = price_per_unit * quantity as u64;
        let deposit = match deposit_tier.as_str() {
            "2-Day Reserve (10%)" => total / 10,
            "1-Week Reserve (20%)" => total / 5,
            _ => total, // Full Payment
        };

        self.record_debit(
            deposit,
            format!("Order deposit: {} x{}", product, quantity),
        )?;

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
        Some(id)
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
