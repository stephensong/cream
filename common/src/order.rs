use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};

use crate::identity::CustomerId;
use crate::product::ProductId;

/// Unique order identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OrderId(pub String);

/// How much deposit the customer puts down to reserve a product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepositTier {
    /// 10% deposit, hold for 2 days.
    Reserve2Days,
    /// 20% deposit, hold for 1 week.
    Reserve1Week,
    /// 100% payment, hold until product expiry.
    FullPayment,
}

impl DepositTier {
    /// Deposit percentage as a fraction (0.0 - 1.0).
    pub fn deposit_fraction(self) -> f64 {
        match self {
            DepositTier::Reserve2Days => 0.10,
            DepositTier::Reserve1Week => 0.20,
            DepositTier::FullPayment => 1.00,
        }
    }

    /// Calculate the required deposit amount for a given total price.
    pub fn calculate_deposit(self, total_price: u64) -> u64 {
        match self {
            DepositTier::Reserve2Days => total_price / 10,
            DepositTier::Reserve1Week => total_price / 5,
            DepositTier::FullPayment => total_price,
        }
    }
}

/// Monotonic order status. Higher ordinal always wins in merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Reservation active until expiry.
    Reserved { expires_at: DateTime<Utc> },
    /// Full payment received.
    Paid,
    /// Product handed over to customer.
    Fulfilled,
    /// Order cancelled (by customer or supplier).
    Cancelled,
    /// Reservation expired without payment.
    Expired,
}

impl OrderStatus {
    /// Ordinal for determining merge winner. Higher always wins.
    pub fn ordinal(&self) -> u8 {
        match self {
            OrderStatus::Reserved { .. } => 0,
            OrderStatus::Paid => 1,
            OrderStatus::Cancelled => 2,
            OrderStatus::Expired => 2,
            OrderStatus::Fulfilled => 3,
        }
    }

    /// Returns true if transitioning from self to `next` is valid.
    pub fn can_transition_to(&self, next: &OrderStatus) -> bool {
        matches!(
            (self, next),
            (OrderStatus::Reserved { .. }, OrderStatus::Paid)
                | (OrderStatus::Reserved { .. }, OrderStatus::Cancelled)
                | (OrderStatus::Reserved { .. }, OrderStatus::Expired)
                | (OrderStatus::Paid, OrderStatus::Fulfilled)
                | (OrderStatus::Paid, OrderStatus::Cancelled)
        )
    }
}

/// An order placed by a customer for a product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub product_id: ProductId,
    pub customer: CustomerId,
    pub quantity: u32,
    pub deposit_tier: DepositTier,
    pub deposit_amount: u64,
    pub total_price: u64,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    /// Customer's signature over the order data.
    pub signature: Signature,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deposit_calculation() {
        assert_eq!(DepositTier::Reserve2Days.calculate_deposit(1000), 100);
        assert_eq!(DepositTier::Reserve1Week.calculate_deposit(1000), 200);
        assert_eq!(DepositTier::FullPayment.calculate_deposit(1000), 1000);
    }

    #[test]
    fn test_status_transitions() {
        let reserved = OrderStatus::Reserved {
            expires_at: Utc::now(),
        };
        assert!(reserved.can_transition_to(&OrderStatus::Paid));
        assert!(reserved.can_transition_to(&OrderStatus::Cancelled));
        assert!(reserved.can_transition_to(&OrderStatus::Expired));
        assert!(!reserved.can_transition_to(&OrderStatus::Fulfilled));

        assert!(OrderStatus::Paid.can_transition_to(&OrderStatus::Fulfilled));
        assert!(OrderStatus::Paid.can_transition_to(&OrderStatus::Cancelled));
        assert!(!OrderStatus::Paid.can_transition_to(&OrderStatus::Expired));

        assert!(!OrderStatus::Fulfilled.can_transition_to(&OrderStatus::Cancelled));
    }

    #[test]
    fn test_status_ordinals_monotonic() {
        let reserved = OrderStatus::Reserved {
            expires_at: Utc::now(),
        };
        assert!(reserved.ordinal() < OrderStatus::Paid.ordinal());
        assert!(OrderStatus::Paid.ordinal() < OrderStatus::Fulfilled.ordinal());
        assert!(reserved.ordinal() < OrderStatus::Cancelled.ordinal());
        assert!(reserved.ordinal() < OrderStatus::Expired.ordinal());
    }
}
