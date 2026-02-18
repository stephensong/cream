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

/// A product listed by the current user (as supplier).
#[derive(Clone, Debug, PartialEq)]
pub struct ListedProduct {
    pub id: u32,
    pub name: String,
    pub category: String,
    pub description: String,
    pub price_curd: u64,
    pub quantity_available: u32,
}

/// Shared application state accessible from all components.
#[derive(Clone, Debug)]
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
        id
    }

    pub fn remove_product(&mut self, id: u32) {
        self.products.retain(|p| p.id != id);
    }
}

/// Provide UserState as shared context at the top of the app.
pub fn use_user_state() -> Signal<UserState> {
    use_context::<Signal<UserState>>()
}
