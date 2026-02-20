use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use cream_common::directory::DirectoryEntry;
use cream_common::identity::{CustomerId, SupplierId};
use cream_common::order::Order;
use cream_common::product::Product;
use cream_common::storefront::order_signable_bytes;

const SUPPLIER_INFO: &[u8] = b"cream-supplier-signing-key-v1";
const CUSTOMER_INFO: &[u8] = b"cream-customer-signing-key-v1";

#[cfg(target_family = "wasm")]
const STORAGE_KEY_SEED: &str = "cream_encrypted_seed";
#[cfg(target_family = "wasm")]
const STORAGE_KEY_SALT: &str = "cream_kdf_salt";

/// Manages cryptographic identity derived from a BIP39 mnemonic.
#[derive(Clone)]
pub struct KeyManager {
    supplier_signing_key: SigningKey,
    customer_signing_key: SigningKey,
}

#[derive(Debug)]
pub enum KeyManagerError {
    InvalidMnemonic,
    DerivationFailed,
    EncryptionFailed,
    DecryptionFailed,
    StorageUnavailable,
    RngFailed,
    NoStoredIdentity,
}

impl fmt::Display for KeyManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMnemonic => write!(f, "Invalid mnemonic phrase"),
            Self::DerivationFailed => write!(f, "Key derivation failed"),
            Self::EncryptionFailed => write!(f, "Encryption failed"),
            Self::DecryptionFailed => write!(f, "Wrong password or corrupted data"),
            Self::StorageUnavailable => write!(f, "Browser storage unavailable"),
            Self::RngFailed => write!(f, "Random number generation failed"),
            Self::NoStoredIdentity => write!(f, "No stored identity found"),
        }
    }
}

impl KeyManager {
    /// Check if there is an encrypted identity in localStorage.
    pub fn has_stored_identity() -> bool {
        #[cfg(target_family = "wasm")]
        {
            get_local_storage_item(STORAGE_KEY_SEED).is_some()
        }
        #[cfg(not(target_family = "wasm"))]
        {
            false
        }
    }

    /// Generate a new random 12-word BIP39 mnemonic.
    pub fn generate_mnemonic() -> Result<bip39::Mnemonic, KeyManagerError> {
        use rand::RngCore;
        let mut entropy = [0u8; 16]; // 128 bits = 12 words
        rand::thread_rng()
            .try_fill_bytes(&mut entropy)
            .map_err(|_| KeyManagerError::RngFailed)?;
        Ok(bip39::Mnemonic::from_entropy(&entropy).map_err(|_| KeyManagerError::DerivationFailed)?)
    }

    /// Derive supplier + customer keys from a mnemonic.
    pub fn from_mnemonic(mnemonic: &bip39::Mnemonic) -> Result<Self, KeyManagerError> {
        let seed = mnemonic.to_seed("");
        Self::from_seed(&seed)
    }

    /// Encrypt the mnemonic's seed and save to localStorage.
    pub fn save_encrypted(
        mnemonic: &bip39::Mnemonic,
        password: &str,
    ) -> Result<(), KeyManagerError> {
        let seed = mnemonic.to_seed("");
        encrypt_and_store(&seed, password)
    }

    /// Decrypt the stored seed with the given password and derive keys.
    pub fn unlock(password: &str) -> Result<Self, KeyManagerError> {
        let seed = decrypt_stored(password)?;
        Self::from_seed(&seed)
    }

    /// Remove encrypted identity from localStorage.
    pub fn clear_stored_identity() {
        #[cfg(target_family = "wasm")]
        {
            if let Some(storage) = get_local_storage() {
                let _ = storage.remove_item(STORAGE_KEY_SEED);
                let _ = storage.remove_item(STORAGE_KEY_SALT);
            }
        }
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
        // Temporarily set a zero signature so signable_bytes is deterministic
        entry.signature = Signature::from_bytes(&[0u8; 64]);
        let bytes = entry.signable_bytes();
        entry.signature = self.supplier_signing_key.sign(&bytes);
    }

    /// Sign an order in-place (as customer).
    pub fn sign_order(&self, order: &mut Order) {
        let bytes = order_signable_bytes(order);
        order.signature = self.customer_signing_key.sign(&bytes);
    }

    fn from_seed(seed: &[u8]) -> Result<Self, KeyManagerError> {
        let supplier_key_bytes = derive_key_material(seed, SUPPLIER_INFO)?;
        let customer_key_bytes = derive_key_material(seed, CUSTOMER_INFO)?;

        Ok(Self {
            supplier_signing_key: SigningKey::from_bytes(&supplier_key_bytes),
            customer_signing_key: SigningKey::from_bytes(&customer_key_bytes),
        })
    }
}

/// Derive 32 bytes from a seed using HKDF-SHA256.
fn derive_key_material(seed: &[u8], info: &[u8]) -> Result<[u8; 32], KeyManagerError> {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(None, seed);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm)
        .map_err(|_| KeyManagerError::DerivationFailed)?;
    Ok(okm)
}

/// Encrypt the 64-byte seed with Argon2id + ChaCha20Poly1305 and store in localStorage.
fn encrypt_and_store(seed: &[u8; 64], password: &str) -> Result<(), KeyManagerError> {
    use argon2::Argon2;
    use base64::prelude::*;
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Nonce};
    use rand::RngCore;

    // Generate random salt (16 bytes) and nonce (12 bytes)
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    let mut rng = rand::thread_rng();
    rng.try_fill_bytes(&mut salt)
        .map_err(|_| KeyManagerError::RngFailed)?;
    rng.try_fill_bytes(&mut nonce_bytes)
        .map_err(|_| KeyManagerError::RngFailed)?;

    // Derive encryption key from password + salt
    let mut enc_key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut enc_key)
        .map_err(|_| KeyManagerError::EncryptionFailed)?;

    // Encrypt seed
    let cipher =
        ChaCha20Poly1305::new_from_slice(&enc_key).map_err(|_| KeyManagerError::EncryptionFailed)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, seed.as_ref())
        .map_err(|_| KeyManagerError::EncryptionFailed)?;

    // Store: nonce || ciphertext (base64), and salt (base64)
    let mut blob = Vec::with_capacity(12 + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    set_local_storage_item(STORAGE_KEY_SEED, &BASE64_STANDARD.encode(&blob))?;
    set_local_storage_item(STORAGE_KEY_SALT, &BASE64_STANDARD.encode(&salt))?;

    Ok(())
}

/// Decrypt the stored seed using password.
fn decrypt_stored(password: &str) -> Result<[u8; 64], KeyManagerError> {
    use argon2::Argon2;
    use base64::prelude::*;
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Nonce};

    let blob_b64 = get_local_storage_item(STORAGE_KEY_SEED)
        .ok_or(KeyManagerError::NoStoredIdentity)?;
    let salt_b64 = get_local_storage_item(STORAGE_KEY_SALT)
        .ok_or(KeyManagerError::NoStoredIdentity)?;

    let blob = BASE64_STANDARD
        .decode(&blob_b64)
        .map_err(|_| KeyManagerError::DecryptionFailed)?;
    let salt = BASE64_STANDARD
        .decode(&salt_b64)
        .map_err(|_| KeyManagerError::DecryptionFailed)?;

    if blob.len() < 12 {
        return Err(KeyManagerError::DecryptionFailed);
    }

    let (nonce_bytes, ciphertext) = blob.split_at(12);

    // Derive encryption key from password + salt
    let mut enc_key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut enc_key)
        .map_err(|_| KeyManagerError::DecryptionFailed)?;

    // Decrypt
    let cipher =
        ChaCha20Poly1305::new_from_slice(&enc_key).map_err(|_| KeyManagerError::DecryptionFailed)?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| KeyManagerError::DecryptionFailed)?;

    if plaintext.len() != 64 {
        return Err(KeyManagerError::DecryptionFailed);
    }

    let mut seed = [0u8; 64];
    seed.copy_from_slice(&plaintext);
    Ok(seed)
}

// ─── localStorage helpers ────────────────────────────────────────────────────

#[cfg(target_family = "wasm")]
fn get_local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_family = "wasm")]
fn get_local_storage_item(key: &str) -> Option<String> {
    get_local_storage()?.get_item(key).ok()?
}

#[cfg(target_family = "wasm")]
fn set_local_storage_item(key: &str, value: &str) -> Result<(), KeyManagerError> {
    get_local_storage()
        .ok_or(KeyManagerError::StorageUnavailable)?
        .set_item(key, value)
        .map_err(|_| KeyManagerError::StorageUnavailable)
}

#[cfg(not(target_family = "wasm"))]
fn get_local_storage_item(_key: &str) -> Option<String> {
    None
}

#[cfg(not(target_family = "wasm"))]
fn set_local_storage_item(_key: &str, _value: &str) -> Result<(), KeyManagerError> {
    Err(KeyManagerError::StorageUnavailable)
}
