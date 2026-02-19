use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
#[cfg(not(feature = "dev"))]
use ed25519_dalek::Verifier;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::identity::SupplierId;
use crate::location::GeoLocation;
use crate::order::{Order, OrderId};
use crate::product::{Product, ProductId};

/// Signed product listing (supplier must sign).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedProduct {
    pub product: Product,
    pub signature: Signature,
}

impl SignedProduct {
    /// Serialize the product for signing/verification.
    pub fn signable_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.product).expect("serialization should not fail")
    }

    pub fn verify_signature(&self, owner: &VerifyingKey) -> bool {
        #[cfg(feature = "dev")]
        {
            let _ = owner;
            return true;
        }
        #[cfg(not(feature = "dev"))]
        {
            let msg = self.signable_bytes();
            owner.verify(&msg, &self.signature).is_ok()
        }
    }
}

/// Basic information about a storefront.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorefrontInfo {
    pub owner: SupplierId,
    pub name: String,
    pub description: String,
    pub location: GeoLocation,
}

/// Parameters that make each storefront contract unique.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorefrontParameters {
    pub owner: VerifyingKey,
}

/// The full storefront state: info + products + orders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorefrontState {
    pub info: StorefrontInfo,
    pub products: BTreeMap<ProductId, SignedProduct>,
    pub orders: BTreeMap<OrderId, Order>,
}

impl StorefrontState {
    /// Merge another storefront state into this one.
    ///
    /// - Products: LWW by `updated_at`
    /// - Orders: set-union, monotonic status (higher ordinal wins)
    pub fn merge(&mut self, other: StorefrontState) {
        // Merge products (LWW by updated_at)
        for (id, signed) in other.products {
            match self.products.get(&id) {
                Some(existing) if existing.product.updated_at >= signed.product.updated_at => {
                    // Keep existing
                }
                _ => {
                    self.products.insert(id, signed);
                }
            }
        }

        // Merge orders (union + monotonic status)
        for (id, order) in other.orders {
            match self.orders.get(&id) {
                Some(existing) if existing.status.ordinal() >= order.status.ordinal() => {
                    // Keep existing (higher or equal status)
                }
                _ => {
                    self.orders.insert(id, order);
                }
            }
        }
    }

    /// Validate all products are signed by the owner and orders are signed by customers.
    pub fn validate(&self, owner: &VerifyingKey) -> bool {
        #[cfg(feature = "dev")]
        {
            let _ = owner;
            return true;
        }
        #[cfg(not(feature = "dev"))]
        {
            // All products must be signed by the storefront owner
            for signed in self.products.values() {
                if !signed.verify_signature(owner) {
                    return false;
                }
            }

            // All orders must be signed by the customer
            for order in self.orders.values() {
                let msg = order_signable_bytes(order);
                if order.customer.0.verify(&msg, &order.signature).is_err() {
                    return false;
                }

                // Verify deposit amount matches tier
                let expected_deposit = order.deposit_tier.calculate_deposit(order.total_price);
                if order.deposit_amount != expected_deposit {
                    return false;
                }
            }

            true
        }
    }
}

/// Serialize order fields for signing (everything except signature).
pub fn order_signable_bytes(order: &Order) -> Vec<u8> {
    let signable = SignableOrder {
        id: &order.id,
        product_id: &order.product_id,
        customer: &order.customer,
        quantity: order.quantity,
        deposit_tier: &order.deposit_tier,
        total_price: order.total_price,
        created_at: &order.created_at,
    };
    serde_json::to_vec(&signable).expect("serialization should not fail")
}

#[derive(Serialize)]
struct SignableOrder<'a> {
    id: &'a OrderId,
    product_id: &'a ProductId,
    customer: &'a crate::identity::CustomerId,
    quantity: u32,
    deposit_tier: &'a crate::order::DepositTier,
    total_price: u64,
    created_at: &'a DateTime<Utc>,
}

/// Summary of storefront state: IDs -> timestamps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorefrontSummary {
    pub product_timestamps: BTreeMap<ProductId, DateTime<Utc>>,
    pub order_timestamps: BTreeMap<OrderId, (DateTime<Utc>, u8)>, // (created_at, status_ordinal)
}

impl StorefrontState {
    pub fn summarize(&self) -> StorefrontSummary {
        StorefrontSummary {
            product_timestamps: self
                .products
                .iter()
                .map(|(id, sp)| (id.clone(), sp.product.updated_at))
                .collect(),
            order_timestamps: self
                .orders
                .iter()
                .map(|(id, o)| (id.clone(), (o.created_at, o.status.ordinal())))
                .collect(),
        }
    }

    /// Compute delta: products newer than summary, orders with higher status or missing.
    pub fn delta(&self, summary: &StorefrontSummary) -> StorefrontState {
        let products = self
            .products
            .iter()
            .filter(|(id, sp)| {
                summary
                    .product_timestamps
                    .get(*id)
                    .is_none_or(|ts| sp.product.updated_at > *ts)
            })
            .map(|(id, sp)| (id.clone(), sp.clone()))
            .collect();

        let orders = self
            .orders
            .iter()
            .filter(|(id, order)| {
                summary
                    .order_timestamps
                    .get(*id)
                    .is_none_or(|(_, ord)| order.status.ordinal() > *ord)
            })
            .map(|(id, o)| (id.clone(), o.clone()))
            .collect();

        StorefrontState {
            info: self.info.clone(),
            products,
            orders,
        }
    }
}
