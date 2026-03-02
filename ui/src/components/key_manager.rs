use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use cream_common::directory::DirectoryEntry;
use cream_common::identity::UserId;
use cream_common::order::Order;
use cream_common::product::Product;
use cream_common::storefront::order_signable_bytes;

/// Manages cryptographic identity derived from name + password credentials.
///
/// In dev mode, keys are deterministically derived via HKDF so that the same
/// name+password always yields the same keypair (matching test fixture data).
#[derive(Clone)]
pub struct KeyManager {
    signing_key: SigningKey,
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

#[allow(dead_code)] // methods used in WASM builds (node_api::wasm_impl)
impl KeyManager {
    /// Derive a single unified key from name and password.
    ///
    /// Uses the same derivation as the test harness so that logging in as
    /// "Gary" with password "gary" yields the same keys as the fixture data.
    pub fn from_credentials(name: &str, password: &str) -> Result<Self, KeyManagerError> {
        let signing_key = cream_common::identity::derive_user_signing_key(name, password);
        Ok(Self { signing_key })
    }

    pub fn user_id(&self) -> UserId {
        UserId(self.verifying_key())
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey::from(&self.signing_key)
    }

    /// Sign a product listing.
    pub fn sign_product(&self, product: &Product) -> Signature {
        let bytes = serde_json::to_vec(product).expect("serialization should not fail");
        self.signing_key.sign(&bytes)
    }

    /// Sign a directory entry in-place.
    pub fn sign_directory_entry(&self, entry: &mut DirectoryEntry) {
        entry.signature = Signature::from_bytes(&[0u8; 64]);
        let bytes = entry.signable_bytes();
        entry.signature = self.signing_key.sign(&bytes);
    }

    /// Sign an order in-place.
    pub fn sign_order(&self, order: &mut Order) {
        let bytes = order_signable_bytes(order);
        order.signature = self.signing_key.sign(&bytes);
    }

    /// Sign arbitrary bytes. Returns the 64-byte signature.
    pub fn sign_raw(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key.sign(message).to_bytes()
    }

    /// Returns the 32-byte signing key bytes (for chat relay auth).
    pub fn signing_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Returns the public key as a hex string (for chat relay identity).
    pub fn pubkey_hex(&self) -> String {
        let vk = self.verifying_key();
        vk.as_bytes().iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Sign a user contract state update.
    pub fn sign_user_contract(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }
}
