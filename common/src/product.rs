use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique product identifier (timestamp-based, monotonically increasing).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProductId(pub String);

/// Category of raw dairy product.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProductCategory {
    Milk,
    Cheese,
    Butter,
    Cream,
    Yogurt,
    Kefir,
    Other(String),
}

/// A product listing in a supplier's storefront.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: ProductId,
    pub name: String,
    pub description: String,
    pub category: ProductCategory,
    /// Price in smallest CURD unit.
    pub price_curd: u64,
    pub quantity_total: u32,
    pub expiry_date: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
