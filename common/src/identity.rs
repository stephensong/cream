use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// A user's public identity — unified key for all roles (supplier, customer, both).
///
/// Custom serde impl encodes the 32-byte key as a hex string so it works as a
/// JSON map key (JSON requires string keys).
#[derive(Debug, Clone)]
pub struct UserId(pub VerifyingKey);

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0.as_bytes() {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl Serialize for UserId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for UserId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.len() != 64 {
            return Err(serde::de::Error::custom("UserId hex must be 64 chars"));
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hex = std::str::from_utf8(chunk).map_err(serde::de::Error::custom)?;
            bytes[i] = u8::from_str_radix(hex, 16).map_err(serde::de::Error::custom)?;
        }
        VerifyingKey::from_bytes(&bytes)
            .map(UserId)
            .map_err(serde::de::Error::custom)
    }
}

impl PartialEq for UserId {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}
impl Eq for UserId {}

impl PartialOrd for UserId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for UserId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}
impl Hash for UserId {
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
    pub user_id: UserId,
}

/// Derive a deterministic ed25519 signing key from a name and password.
///
/// Uses a single unified derivation domain (`cream-user-signing-key-v1`)
/// so that every user has one key regardless of role.
///
/// Only available with the `dev` feature (production will use BIP39 mnemonics).
#[cfg(feature = "dev")]
pub fn derive_user_signing_key(name: &str, password: &str) -> ed25519_dalek::SigningKey {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let salt = name.trim().to_lowercase();
    let hk = Hkdf::<Sha256>::new(Some(salt.as_bytes()), password.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(b"cream-user-signing-key-v1", &mut okm)
        .expect("HKDF expand should not fail for 32 bytes");
    ed25519_dalek::SigningKey::from_bytes(&okm)
}

/// Name for the root user (Fedimint guardians). Prefixed with `__` to prevent
/// collision with real user names.
pub const ROOT_USER_NAME: &str = "__cream_root__";

/// Derive a deterministic signing key for the root user.
///
/// The root user represents the Fedimint guardians — the source of all CURD.
/// Its key is deterministic so every client can compute the same contract key.
#[cfg(feature = "dev")]
pub fn root_signing_key() -> ed25519_dalek::SigningKey {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let salt = ROOT_USER_NAME.trim().to_lowercase();
    let hk = Hkdf::<Sha256>::new(Some(salt.as_bytes()), b"cream-root-genesis");
    let mut okm = [0u8; 32];
    hk.expand(b"cream-root-signing-key-v1", &mut okm)
        .expect("HKDF expand should not fail for 32 bytes");
    ed25519_dalek::SigningKey::from_bytes(&okm)
}

/// Get the root user's UserId (public key).
///
/// When the `frost` feature is enabled, this returns the FROST group key
/// (threshold signature). Otherwise falls back to the single-signer key.
#[cfg(feature = "dev")]
pub fn root_user_id() -> UserId {
    #[cfg(feature = "frost")]
    {
        UserId(crate::frost::dev_root_verifying_key())
    }
    #[cfg(not(feature = "frost"))]
    {
        UserId(root_signing_key().verifying_key())
    }
}

/// Sign a message as the root user using FROST threshold signatures.
///
/// In trusted-dealer mode, all key shares are held locally and signing
/// is performed in a single process. Production will use distributed
/// guardian signing.
#[cfg(all(feature = "dev", feature = "frost"))]
pub fn root_sign(message: &[u8]) -> ed25519_dalek::Signature {
    let (keys, pkg) = crate::frost::dev_root_frost_keys();
    crate::frost::sign_with_threshold(message, &keys, &pkg, 2)
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
