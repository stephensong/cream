use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// A supplier's public identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierId(pub VerifyingKey);

impl PartialEq for SupplierId {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}
impl Eq for SupplierId {}

impl PartialOrd for SupplierId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SupplierId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}
impl Hash for SupplierId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_bytes().hash(state);
    }
}

/// A customer's public identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerId(pub VerifyingKey);

impl PartialEq for CustomerId {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}
impl Eq for CustomerId {}

impl PartialOrd for CustomerId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for CustomerId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}
impl Hash for CustomerId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_bytes().hash(state);
    }
}

/// Role a user can have in the CREAM marketplace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    Supplier,
    Customer,
    Both,
}

/// Full user identity (for local storage by the delegate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserIdentity {
    pub role: UserRole,
    pub supplier_id: Option<SupplierId>,
    pub customer_id: Option<CustomerId>,
}

/// A value signed by a known key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signed<T> {
    pub data: T,
    pub signature: Signature,
}

impl<T> Signed<T> {
    pub fn verify(&self, key: &VerifyingKey, message: &[u8]) -> bool {
        use ed25519_dalek::Verifier;
        key.verify(message, &self.signature).is_ok()
    }
}
