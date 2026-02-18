use dioxus::prelude::*;

/// A placed order tracked in the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedOrder {
    pub id: u32,
    pub supplier: String,
    pub product: String,
    pub quantity: u32,
    pub deposit_tier: String,
    pub price_per_unit: u64,
    pub status: String,
}

/// Shared application state accessible from all components.
#[derive(Clone, Debug)]
pub struct UserState {
    pub moniker: Option<String>,
    pub orders: Vec<PlacedOrder>,
    pub next_order_id: u32,
}

impl UserState {
    pub fn new() -> Self {
        Self {
            moniker: None,
            orders: Vec::new(),
            next_order_id: 1,
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
        id
    }
}

/// Provide UserState as shared context at the top of the app.
pub fn use_user_state() -> Signal<UserState> {
    use_context::<Signal<UserState>>()
}
