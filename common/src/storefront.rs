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

use crate::order::OrderStatus;

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
            #[allow(clippy::needless_return)]
            return true;
        }
        #[cfg(not(feature = "dev"))]
        {
            let msg = self.signable_bytes();
            owner.verify(&msg, &self.signature).is_ok()
        }
    }
}

/// Weekly opening hours as a bitfield: 7 days × 48 half-hour slots = 336 bits = 42 bytes.
/// Days are Monday (0) through Sunday (6), each day uses 6 bytes (48 bits).
/// Slot 0 = 00:00–00:30, slot 1 = 00:30–01:00, ..., slot 47 = 23:30–00:00.
#[derive(Debug, Clone, PartialEq)]
pub struct WeeklySchedule {
    bits: [u8; 42],
}

// Custom serde: serialize [u8; 42] as a Vec<u8> (serde supports arbitrary-length sequences).
impl Serialize for WeeklySchedule {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.bits)
    }
}

impl<'de> Deserialize<'de> for WeeklySchedule {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        if bytes.len() != 42 {
            return Err(serde::de::Error::custom(format!(
                "expected 42 bytes for WeeklySchedule, got {}",
                bytes.len()
            )));
        }
        let mut bits = [0u8; 42];
        bits.copy_from_slice(&bytes);
        Ok(WeeklySchedule { bits })
    }
}

impl WeeklySchedule {
    /// Create a new schedule with all slots closed.
    pub fn new() -> Self {
        Self { bits: [0u8; 42] }
    }

    /// Check if a specific half-hour slot is open.
    /// `day` 0–6 (Mon–Sun), `slot` 0–47.
    pub fn is_open(&self, day: u8, slot: u8) -> bool {
        if day > 6 || slot > 47 {
            return false;
        }
        let byte_idx = (day as usize) * 6 + (slot as usize) / 8;
        let bit_idx = (slot as usize) % 8;
        self.bits[byte_idx] & (1 << bit_idx) != 0
    }

    /// Set a specific half-hour slot as open or closed.
    pub fn set_slot(&mut self, day: u8, slot: u8, open: bool) {
        if day > 6 || slot > 47 {
            return;
        }
        let byte_idx = (day as usize) * 6 + (slot as usize) / 8;
        let bit_idx = (slot as usize) % 8;
        if open {
            self.bits[byte_idx] |= 1 << bit_idx;
        } else {
            self.bits[byte_idx] &= !(1 << bit_idx);
        }
    }

    /// Set a range of slots (inclusive start, exclusive end) as open or closed.
    pub fn set_range(&mut self, day: u8, start_slot: u8, end_slot: u8, open: bool) {
        for slot in start_slot..end_slot {
            self.set_slot(day, slot, open);
        }
    }

    /// Extract contiguous open ranges for a day. Returns `Vec<(start_slot, end_slot)>`
    /// where end_slot is exclusive.
    pub fn get_ranges(&self, day: u8) -> Vec<(u8, u8)> {
        let mut ranges = Vec::new();
        let mut start: Option<u8> = None;
        for slot in 0..48u8 {
            if self.is_open(day, slot) {
                if start.is_none() {
                    start = Some(slot);
                }
            } else if let Some(s) = start.take() {
                ranges.push((s, slot));
            }
        }
        if let Some(s) = start {
            ranges.push((s, 48));
        }
        ranges
    }

    /// Check if the schedule is currently open given a UTC offset in minutes.
    #[cfg(feature = "std")]
    pub fn is_currently_open(&self, utc_offset_minutes: i32) -> bool {
        let now = chrono::Utc::now();
        self.is_open_at(now, utc_offset_minutes)
    }

    /// Check if the schedule is open at a specific UTC time given an offset.
    pub fn is_open_at(&self, utc_time: DateTime<Utc>, utc_offset_minutes: i32) -> bool {
        let local_timestamp = utc_time.timestamp() + (utc_offset_minutes as i64) * 60;
        let secs_in_day = local_timestamp.rem_euclid(86400);
        let slot = (secs_in_day / 1800) as u8; // 1800 seconds = 30 minutes

        // Weekday: Monday=0 through Sunday=6
        // chrono: Monday=0 (num_days_from_monday)
        let day_timestamp = local_timestamp.div_euclid(86400);
        // Unix epoch (1970-01-01) was a Thursday = day 3 (Mon=0)
        let weekday = ((day_timestamp + 3) % 7) as u8;

        self.is_open(weekday, slot)
    }

    /// Convert a slot index to (hour, minute).
    pub fn slot_to_time(slot: u8) -> (u8, u8) {
        let hour = slot / 2;
        let minute = (slot % 2) * 30;
        (hour, minute)
    }

    /// Convert (hour, minute) to the nearest slot index.
    pub fn time_to_slot(hour: u8, minute: u8) -> u8 {
        hour * 2 + if minute >= 30 { 1 } else { 0 }
    }

    /// Format a slot as "HH:MM" (24-hour).
    pub fn format_slot_24h(slot: u8) -> String {
        let (h, m) = Self::slot_to_time(slot);
        format!("{h:02}:{m:02}")
    }

    /// Format a slot as "H:MM AM/PM".
    pub fn format_slot_12h(slot: u8) -> String {
        let (h, m) = Self::slot_to_time(slot);
        let (h12, ampm) = if h == 0 {
            (12, "AM")
        } else if h < 12 {
            (h, "AM")
        } else if h == 12 {
            (12, "PM")
        } else {
            (h - 12, "PM")
        };
        format!("{h12}:{m:02} {ampm}")
    }

    /// Day name from index (0=Monday).
    pub fn day_name(day: u8) -> &'static str {
        match day {
            0 => "Monday",
            1 => "Tuesday",
            2 => "Wednesday",
            3 => "Thursday",
            4 => "Friday",
            5 => "Saturday",
            6 => "Sunday",
            _ => "Unknown",
        }
    }

    /// Short day name from index (0=Mon).
    pub fn day_name_short(day: u8) -> &'static str {
        match day {
            0 => "Mon",
            1 => "Tue",
            2 => "Wed",
            3 => "Thu",
            4 => "Fri",
            5 => "Sat",
            6 => "Sun",
            _ => "???",
        }
    }
}

impl Default for WeeklySchedule {
    fn default() -> Self {
        Self::new()
    }
}

/// Basic information about a storefront.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorefrontInfo {
    pub owner: SupplierId,
    pub name: String,
    pub description: String,
    pub location: GeoLocation,
    #[serde(default)]
    pub schedule: Option<WeeklySchedule>,
    #[serde(default)]
    pub timezone: Option<String>,
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
    /// Compute available quantity for a product by subtracting active order quantities.
    ///
    /// Active orders are those with status `Reserved` or `Paid` (ordinals 0 and 1).
    pub fn available_quantity(&self, product_id: &ProductId) -> u32 {
        let total = self
            .products
            .get(product_id)
            .map(|sp| sp.product.quantity_total)
            .unwrap_or(0);

        let reserved: u32 = self
            .orders
            .values()
            .filter(|o| {
                o.product_id == *product_id
                    && matches!(o.status, OrderStatus::Reserved { .. } | OrderStatus::Paid)
            })
            .map(|o| o.quantity)
            .sum();

        total.saturating_sub(reserved)
    }

    /// Transition all `Reserved` orders whose `expires_at` has passed to `Expired`.
    /// Returns `true` if any orders were changed.
    pub fn expire_orders(&mut self, now: DateTime<Utc>) -> bool {
        let mut changed = false;
        for order in self.orders.values_mut() {
            if let OrderStatus::Reserved { expires_at } = order.status {
                if expires_at < now {
                    order.status = OrderStatus::Expired;
                    changed = true;
                }
            }
        }
        changed
    }

    /// Merge another storefront state into this one.
    ///
    /// - Products: LWW by `updated_at`
    /// - Orders: set-union, monotonic status (higher ordinal wins)
    pub fn merge(&mut self, other: StorefrontState) {
        // Merge info: single-owner, always take update's info so schedule/timezone
        // and other metadata changes propagate.
        self.info = other.info;

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
            #[allow(clippy::needless_return)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{CustomerId, SupplierId};
    use crate::order::{DepositTier, Order, OrderId};
    use crate::product::ProductId;
    use chrono::{Duration, Utc};
    use ed25519_dalek::{SigningKey, Signature};

    fn dummy_storefront() -> StorefrontState {
        let key = SigningKey::from_bytes(&[1u8; 32]);
        StorefrontState {
            info: StorefrontInfo {
                owner: SupplierId(key.verifying_key()),
                name: "Test Farm".into(),
                description: "".into(),
                location: GeoLocation::new(0.0, 0.0),
                schedule: None,
                timezone: None,
            },
            products: BTreeMap::new(),
            orders: BTreeMap::new(),
        }
    }

    fn dummy_order(id: &str, status: OrderStatus) -> Order {
        let key = SigningKey::from_bytes(&[2u8; 32]);
        Order {
            id: OrderId(id.into()),
            product_id: ProductId("p-1".into()),
            customer: CustomerId(key.verifying_key()),
            quantity: 1,
            deposit_tier: DepositTier::Reserve2Days,
            deposit_amount: 10,
            total_price: 100,
            status,
            created_at: Utc::now(),
            signature: Signature::from_bytes(&[0u8; 64]),
        }
    }

    #[test]
    fn expire_orders_transitions_past_reserved() {
        let mut sf = dummy_storefront();
        let yesterday = Utc::now() - Duration::days(1);
        let tomorrow = Utc::now() + Duration::days(1);

        sf.orders.insert(
            OrderId("expired".into()),
            dummy_order("expired", OrderStatus::Reserved { expires_at: yesterday }),
        );
        sf.orders.insert(
            OrderId("active".into()),
            dummy_order("active", OrderStatus::Reserved { expires_at: tomorrow }),
        );
        sf.orders.insert(
            OrderId("paid".into()),
            dummy_order("paid", OrderStatus::Paid),
        );

        let changed = sf.expire_orders(Utc::now());
        assert!(changed);
        assert_eq!(sf.orders[&OrderId("expired".into())].status, OrderStatus::Expired);
        assert!(matches!(sf.orders[&OrderId("active".into())].status, OrderStatus::Reserved { .. }));
        assert_eq!(sf.orders[&OrderId("paid".into())].status, OrderStatus::Paid);
    }

    #[test]
    fn weekly_schedule_new_is_all_closed() {
        let sched = WeeklySchedule::new();
        for day in 0..7 {
            for slot in 0..48 {
                assert!(!sched.is_open(day, slot), "day {day} slot {slot} should be closed");
            }
        }
    }

    #[test]
    fn weekly_schedule_set_and_get_slot() {
        let mut sched = WeeklySchedule::new();
        sched.set_slot(0, 16, true); // Monday 8:00 AM
        assert!(sched.is_open(0, 16));
        assert!(!sched.is_open(0, 15));
        assert!(!sched.is_open(1, 16)); // Tuesday same slot should be closed

        sched.set_slot(0, 16, false);
        assert!(!sched.is_open(0, 16));
    }

    #[test]
    fn weekly_schedule_set_range() {
        let mut sched = WeeklySchedule::new();
        // Monday 8:00 AM (slot 16) to 5:00 PM (slot 34)
        sched.set_range(0, 16, 34, true);
        assert!(!sched.is_open(0, 15));
        assert!(sched.is_open(0, 16));
        assert!(sched.is_open(0, 33));
        assert!(!sched.is_open(0, 34));
    }

    #[test]
    fn weekly_schedule_get_ranges() {
        let mut sched = WeeklySchedule::new();
        sched.set_range(0, 16, 34, true); // 8:00–17:00
        sched.set_range(0, 38, 42, true); // 19:00–21:00

        let ranges = sched.get_ranges(0);
        assert_eq!(ranges, vec![(16, 34), (38, 42)]);

        assert!(sched.get_ranges(1).is_empty());
    }

    #[test]
    fn weekly_schedule_is_open_at() {
        let mut sched = WeeklySchedule::new();
        // Monday 9:00–17:00 (slots 18–34)
        sched.set_range(0, 18, 34, true);

        // 2024-01-01 is a Monday. Test at 10:00 UTC with offset 0 => open
        let monday_10am = chrono::DateTime::parse_from_rfc3339("2024-01-01T10:00:00Z")
            .unwrap().with_timezone(&Utc);
        assert!(sched.is_open_at(monday_10am, 0));

        // Same UTC time but offset +11:00 (= 21:00 local) => closed
        assert!(!sched.is_open_at(monday_10am, 11 * 60));

        // Tuesday same time => closed (no Tuesday hours)
        let tuesday_10am = chrono::DateTime::parse_from_rfc3339("2024-01-02T10:00:00Z")
            .unwrap().with_timezone(&Utc);
        assert!(!sched.is_open_at(tuesday_10am, 0));
    }

    #[test]
    fn weekly_schedule_slot_time_conversion() {
        assert_eq!(WeeklySchedule::slot_to_time(0), (0, 0));
        assert_eq!(WeeklySchedule::slot_to_time(1), (0, 30));
        assert_eq!(WeeklySchedule::slot_to_time(16), (8, 0));
        assert_eq!(WeeklySchedule::slot_to_time(47), (23, 30));

        assert_eq!(WeeklySchedule::time_to_slot(8, 0), 16);
        assert_eq!(WeeklySchedule::time_to_slot(8, 30), 17);
        assert_eq!(WeeklySchedule::time_to_slot(17, 0), 34);
    }

    #[test]
    fn weekly_schedule_format_slots() {
        assert_eq!(WeeklySchedule::format_slot_24h(0), "00:00");
        assert_eq!(WeeklySchedule::format_slot_24h(16), "08:00");
        assert_eq!(WeeklySchedule::format_slot_24h(34), "17:00");

        assert_eq!(WeeklySchedule::format_slot_12h(0), "12:00 AM");
        assert_eq!(WeeklySchedule::format_slot_12h(16), "8:00 AM");
        assert_eq!(WeeklySchedule::format_slot_12h(24), "12:00 PM");
        assert_eq!(WeeklySchedule::format_slot_12h(34), "5:00 PM");
    }

    #[test]
    fn weekly_schedule_boundary_values() {
        let mut sched = WeeklySchedule::new();
        // Out of bounds should be no-ops / return false
        assert!(!sched.is_open(7, 0));
        assert!(!sched.is_open(0, 48));
        sched.set_slot(7, 0, true); // no-op
        sched.set_slot(0, 48, true); // no-op
        assert_eq!(sched.bits, [0u8; 42]);
    }

    #[test]
    fn weekly_schedule_serialization_roundtrip() {
        let mut sched = WeeklySchedule::new();
        sched.set_range(0, 16, 34, true);
        sched.set_range(5, 16, 24, true);

        let json = serde_json::to_string(&sched).unwrap();
        let deserialized: WeeklySchedule = serde_json::from_str(&json).unwrap();
        assert_eq!(sched, deserialized);
    }

    #[test]
    fn storefront_info_backward_compat() {
        // Old JSON without schedule/timezone fields should deserialize fine
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let info_old = StorefrontInfo {
            owner: SupplierId(key.verifying_key()),
            name: "Test".into(),
            description: "desc".into(),
            location: GeoLocation::new(0.0, 0.0),
            schedule: None,
            timezone: None,
        };
        let json = serde_json::to_string(&info_old).unwrap();
        // Remove schedule and timezone fields to simulate old format
        let old_json = json.replace(",\"schedule\":null,\"timezone\":null", "");
        let info: StorefrontInfo = serde_json::from_str(&old_json).unwrap();
        assert!(info.schedule.is_none());
        assert!(info.timezone.is_none());
    }

    #[test]
    fn expire_orders_returns_false_when_nothing_changed() {
        let mut sf = dummy_storefront();
        let tomorrow = Utc::now() + Duration::days(1);
        sf.orders.insert(
            OrderId("active".into()),
            dummy_order("active", OrderStatus::Reserved { expires_at: tomorrow }),
        );

        assert!(!sf.expire_orders(Utc::now()));
    }
}
