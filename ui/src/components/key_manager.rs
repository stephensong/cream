use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use cream_common::directory::DirectoryEntry;
use cream_common::identity::{CustomerId, SupplierId};
use cream_common::order::Order;
use cream_common::product::Product;
use cream_common::storefront::order_signable_bytes;

/// Manages cryptographic identity derived from name + password credentials.
///
/// In dev mode, keys are deterministically derived via HKDF so that the same
/// name+password always yields the same keypair (matching test fixture data).
#[derive(Clone)]
pub struct KeyManager {
    supplier_signing_key: SigningKey,
    #[allow(dead_code)] // used by customer_id/sign_order in use-node builds
    customer_signing_key: SigningKey,
}

#[derive(Debug)]
pub enum KeyManagerError {
    #[allow(dead_code)] // used when key derivation is fallible
    DerivationFailed,
}

impl fmt::Display for KeyManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DerivationFailed => write!(f, "Key derivation failed"),
        }
    }
}

impl KeyManager {
    /// Derive supplier + customer keys from name and password.
    ///
    /// Uses the same derivation as the test harness so that logging in as
    /// "Gary" with password "gary" yields the same keys as the fixture data.
    pub fn from_credentials(name: &str, password: &str) -> Result<Self, KeyManagerError> {
        let supplier_signing_key =
            cream_common::identity::derive_supplier_signing_key(name, password);
        let customer_signing_key =
            cream_common::identity::derive_customer_signing_key(name, password);
        Ok(Self {
            supplier_signing_key,
            customer_signing_key,
        })
    }

    pub fn supplier_id(&self) -> SupplierId {
        SupplierId(self.supplier_verifying_key())
    }

    pub fn customer_id(&self) -> CustomerId {
        CustomerId(self.customer_verifying_key())
    }

    pub fn supplier_verifying_key(&self) -> VerifyingKey {
        VerifyingKey::from(&self.supplier_signing_key)
    }

    pub fn customer_verifying_key(&self) -> VerifyingKey {
        VerifyingKey::from(&self.customer_signing_key)
    }

    /// Sign a product listing.
    pub fn sign_product(&self, product: &Product) -> Signature {
        let bytes = serde_json::to_vec(product).expect("serialization should not fail");
        self.supplier_signing_key.sign(&bytes)
    }

    /// Sign a directory entry in-place.
    pub fn sign_directory_entry(&self, entry: &mut DirectoryEntry) {
        entry.signature = Signature::from_bytes(&[0u8; 64]);
        let bytes = entry.signable_bytes();
        entry.signature = self.supplier_signing_key.sign(&bytes);
    }

    /// Sign an order in-place (as customer).
    pub fn sign_order(&self, order: &mut Order) {
        let bytes = order_signable_bytes(order);
        order.signature = self.customer_signing_key.sign(&bytes);
    }
}
