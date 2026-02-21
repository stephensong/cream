use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use cream_common::directory::DirectoryEntry;
use cream_common::identity::{CustomerId, SupplierId, UserIdentity, UserRole};
use cream_common::order::Order;
use cream_common::product::Product;
use cream_common::storefront::{order_signable_bytes, SignedProduct};

/// Requests that can be sent to the CREAM delegate.
#[derive(Debug, Serialize, Deserialize)]
pub enum CreamRequest {
    // Identity management
    CreateIdentity { role: UserRole },
    GetIdentity,

    // Mock wallet
    GetBalance,
    SetBalance(u64),

    // Signing
    SignProduct(Product),
    SignOrder(Order),
    SignDirectoryEntry(DirectoryEntry),
}

/// Responses from the CREAM delegate.
#[derive(Debug, Serialize, Deserialize)]
pub enum CreamResponse {
    Identity(UserIdentity),
    Balance(u64),
    SignedProduct(SignedProduct),
    SignedOrder(Order),
    SignedDirectoryEntry(DirectoryEntry),
    Error(String),
}

/// Internal state persisted by the delegate.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DelegateState {
    /// Supplier signing key (secret).
    supplier_key: Option<Vec<u8>>,
    /// Customer signing key (secret).
    customer_key: Option<Vec<u8>>,
    /// User role.
    role: Option<UserRole>,
    /// Mock CURD balance.
    balance: u64,
}

impl DelegateState {
    pub fn handle_request(&mut self, request: CreamRequest) -> CreamResponse {
        match request {
            CreamRequest::CreateIdentity { role } => self.create_identity(role),
            CreamRequest::GetIdentity => self.get_identity(),
            CreamRequest::GetBalance => CreamResponse::Balance(self.balance),
            CreamRequest::SetBalance(amount) => {
                self.balance = amount;
                CreamResponse::Balance(self.balance)
            }
            CreamRequest::SignProduct(product) => self.sign_product(product),
            CreamRequest::SignOrder(mut order) => self.sign_order(&mut order),
            CreamRequest::SignDirectoryEntry(mut entry) => self.sign_directory_entry(&mut entry),
        }
    }

    fn create_identity(&mut self, role: UserRole) -> CreamResponse {
        match role {
            UserRole::Supplier => {
                let key = SigningKey::generate(&mut rand::rngs::OsRng);
                self.supplier_key = Some(key.to_bytes().to_vec());
            }
            UserRole::Customer => {
                let key = SigningKey::generate(&mut rand::rngs::OsRng);
                self.customer_key = Some(key.to_bytes().to_vec());
            }
            UserRole::Both => {
                let skey = SigningKey::generate(&mut rand::rngs::OsRng);
                self.supplier_key = Some(skey.to_bytes().to_vec());
                let ckey = SigningKey::generate(&mut rand::rngs::OsRng);
                self.customer_key = Some(ckey.to_bytes().to_vec());
            }
        }
        self.role = Some(role);
        // Give a starting balance for demo purposes
        if self.balance == 0 {
            self.balance = 10_000;
        }
        self.get_identity()
    }

    fn get_identity(&self) -> CreamResponse {
        let role = match &self.role {
            Some(r) => r.clone(),
            None => return CreamResponse::Error("No identity created".into()),
        };

        let supplier_id = self.supplier_key.as_ref().map(|bytes| {
            let key = signing_key_from_bytes(bytes);
            SupplierId(VerifyingKey::from(&key))
        });

        let customer_id = self.customer_key.as_ref().map(|bytes| {
            let key = signing_key_from_bytes(bytes);
            CustomerId(VerifyingKey::from(&key))
        });

        CreamResponse::Identity(UserIdentity {
            role,
            supplier_id,
            customer_id,
        })
    }

    fn sign_product(&self, product: Product) -> CreamResponse {
        let Some(key_bytes) = &self.supplier_key else {
            return CreamResponse::Error("No supplier identity".into());
        };
        let key = signing_key_from_bytes(key_bytes);
        let msg = serde_json::to_vec(&product).expect("serialization should not fail");
        let signature = key.sign(&msg);
        CreamResponse::SignedProduct(SignedProduct { product, signature })
    }

    fn sign_order(&self, order: &mut Order) -> CreamResponse {
        let Some(key_bytes) = &self.customer_key else {
            return CreamResponse::Error("No customer identity".into());
        };
        let key = signing_key_from_bytes(key_bytes);
        let msg = order_signable_bytes(order);
        order.signature = key.sign(&msg);
        CreamResponse::SignedOrder(order.clone())
    }

    fn sign_directory_entry(&self, entry: &mut DirectoryEntry) -> CreamResponse {
        let Some(key_bytes) = &self.supplier_key else {
            return CreamResponse::Error("No supplier identity".into());
        };
        let key = signing_key_from_bytes(key_bytes);
        let msg = entry.signable_bytes();
        entry.signature = key.sign(&msg);
        CreamResponse::SignedDirectoryEntry(entry.clone())
    }
}

fn signing_key_from_bytes(bytes: &[u8]) -> SigningKey {
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&bytes[..32]);
    SigningKey::from_bytes(&key_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cream_common::location::GeoLocation;
    use cream_common::product::{Product, ProductCategory, ProductId};

    #[test]
    fn test_create_identity_and_sign_product() {
        let mut state = DelegateState::default();

        // Create supplier identity
        let resp = state.handle_request(CreamRequest::CreateIdentity {
            role: UserRole::Supplier,
        });
        let identity = match resp {
            CreamResponse::Identity(id) => id,
            other => panic!("Expected Identity, got {:?}", other),
        };
        assert!(identity.supplier_id.is_some());
        assert!(identity.customer_id.is_none());

        // Sign a product
        let product = Product {
            id: ProductId("test-001".into()),
            name: "Raw Milk".into(),
            description: "Fresh raw milk".into(),
            category: ProductCategory::Milk,
            price_curd: 500,
            quantity_available: 10,
            expiry_date: None,
            updated_at: Utc::now(),
            created_at: Utc::now(),
        };

        let resp = state.handle_request(CreamRequest::SignProduct(product));
        let signed = match resp {
            CreamResponse::SignedProduct(sp) => sp,
            other => panic!("Expected SignedProduct, got {:?}", other),
        };

        // Verify signature
        let key_bytes = state.supplier_key.as_ref().unwrap();
        let key = signing_key_from_bytes(key_bytes);
        let verifying_key = VerifyingKey::from(&key);
        assert!(signed.verify_signature(&verifying_key));
    }

    #[test]
    fn test_create_both_identity() {
        let mut state = DelegateState::default();
        let resp = state.handle_request(CreamRequest::CreateIdentity {
            role: UserRole::Both,
        });
        let identity = match resp {
            CreamResponse::Identity(id) => id,
            other => panic!("Expected Identity, got {:?}", other),
        };
        assert!(identity.supplier_id.is_some());
        assert!(identity.customer_id.is_some());
        assert_eq!(identity.role, UserRole::Both);
    }

    #[test]
    fn test_mock_wallet() {
        let mut state = DelegateState::default();
        state.handle_request(CreamRequest::CreateIdentity {
            role: UserRole::Customer,
        });

        // Default balance should be 10_000
        let resp = state.handle_request(CreamRequest::GetBalance);
        assert!(matches!(resp, CreamResponse::Balance(10_000)));

        // Set balance
        let resp = state.handle_request(CreamRequest::SetBalance(50_000));
        assert!(matches!(resp, CreamResponse::Balance(50_000)));
    }

    #[test]
    fn test_sign_directory_entry() {
        let mut state = DelegateState::default();
        state.handle_request(CreamRequest::CreateIdentity {
            role: UserRole::Supplier,
        });

        let identity = match state.handle_request(CreamRequest::GetIdentity) {
            CreamResponse::Identity(id) => id,
            _ => panic!("expected identity"),
        };

        let entry = DirectoryEntry {
            supplier: identity.supplier_id.unwrap(),
            name: "Test Farm".into(),
            description: "A test dairy farm".into(),
            location: GeoLocation::new(40.0, -74.0),
            postcode: None,
            locality: None,
            categories: vec![ProductCategory::Milk, ProductCategory::Cheese],
            storefront_key: freenet_stdlib::prelude::ContractKey::from_params(
                "11111111111111111111111111111111",
                freenet_stdlib::prelude::Parameters::from(vec![]),
            )
            .unwrap(),
            updated_at: Utc::now(),
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
        };

        let resp = state.handle_request(CreamRequest::SignDirectoryEntry(entry));
        let signed_entry = match resp {
            CreamResponse::SignedDirectoryEntry(e) => e,
            other => panic!("Expected SignedDirectoryEntry, got {:?}", other),
        };

        assert!(signed_entry.verify_signature());
    }
}
