use std::collections::BTreeMap;
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest, WebApi};
use freenet_stdlib::prelude::*;

use cream_common::directory::DirectoryState;
use cream_common::identity::{CustomerId, SupplierId};
use cream_common::location::GeoLocation;
use cream_common::product::{Product, ProductCategory, ProductId};
use cream_common::storefront::{SignedProduct, StorefrontInfo, StorefrontState};

use crate::{
    connect_to_node, extract_get_response_state, extract_notification_bytes, is_get_response,
    is_put_response, is_subscribe_success, is_update_notification, is_update_response,
    make_directory_contract, make_directory_entry, make_dummy_customer, make_dummy_supplier,
    make_storefront_contract, recv_matching,
};

const TIMEOUT: Duration = Duration::from_secs(5);

/// A supplier participant in the test harness.
pub struct Supplier {
    pub name: String,
    pub id: SupplierId,
    pub verifying_key: VerifyingKey,
    pub api: WebApi,
    pub storefront_key: ContractKey,
    pub storefront: StorefrontState,
}

impl Supplier {
    /// Add a product to this supplier's storefront, send the update, and wait for confirmation.
    pub async fn add_product(
        &mut self,
        name: &str,
        category: ProductCategory,
        price_curd: u64,
    ) -> &SignedProduct {
        let now = chrono::Utc::now();
        let product = SignedProduct {
            product: Product {
                id: ProductId(format!(
                    "p-{}-{}",
                    self.name.to_lowercase(),
                    now.timestamp_millis()
                )),
                name: name.to_string(),
                description: format!("Fresh {name}"),
                category,
                price_curd,
                quantity_available: 10,
                expiry_date: None,
                updated_at: now,
                created_at: now,
            },
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
        };

        let pid = product.product.id.clone();
        self.storefront.products.insert(pid.clone(), product);

        let sf_bytes = serde_json::to_vec(&self.storefront).unwrap();
        self.api
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: self.storefront_key,
                data: UpdateData::State(State::from(sf_bytes)),
            }))
            .await
            .unwrap();

        // Wait for UpdateResponse confirmation
        recv_matching(&mut self.api, is_update_response, TIMEOUT)
            .await
            .expect("Expected UpdateResponse after adding product");

        // Small delay so next product gets a distinct timestamp-based ID
        tokio::time::sleep(Duration::from_millis(10)).await;

        self.storefront.products.get(&pid).unwrap()
    }

    /// Return a reference to the local storefront state copy.
    pub fn get_storefront_state(&self) -> &StorefrontState {
        &self.storefront
    }

    /// Subscribe to the directory contract.
    pub async fn subscribe_to_directory(&mut self, dir_key: &ContractKey) {
        self.api
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *dir_key.id(),
                summary: None,
            }))
            .await
            .unwrap();

        recv_matching(&mut self.api, is_subscribe_success, TIMEOUT)
            .await
            .expect("Expected SubscribeResponse for directory");
    }
}

/// A customer participant in the test harness.
pub struct Customer {
    pub name: String,
    pub id: CustomerId,
    pub verifying_key: VerifyingKey,
    pub api: WebApi,
}

impl Customer {
    /// GET a supplier's storefront state.
    pub async fn get_storefront(&mut self, supplier: &Supplier) -> StorefrontState {
        self.api
            .send(ClientRequest::ContractOp(ContractRequest::Get {
                key: *supplier.storefront_key.id(),
                return_contract_code: false,
                subscribe: false,
                blocking_subscribe: false,
            }))
            .await
            .unwrap();

        let resp = recv_matching(&mut self.api, is_get_response, TIMEOUT)
            .await
            .expect("Expected GetResponse for storefront");

        let bytes = extract_get_response_state(&resp).expect("state bytes from GET");
        serde_json::from_slice(&bytes).expect("deserialize storefront from GET")
    }

    /// Subscribe to a supplier's storefront contract.
    pub async fn subscribe_to_storefront(&mut self, supplier: &Supplier) {
        self.api
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *supplier.storefront_key.id(),
                summary: None,
            }))
            .await
            .unwrap();

        recv_matching(&mut self.api, is_subscribe_success, TIMEOUT)
            .await
            .expect("Expected SubscribeResponse for storefront");
    }

    /// Subscribe to the directory contract.
    pub async fn subscribe_to_directory(&mut self, dir_key: &ContractKey) {
        self.api
            .send(ClientRequest::ContractOp(ContractRequest::Subscribe {
                key: *dir_key.id(),
                summary: None,
            }))
            .await
            .unwrap();

        recv_matching(&mut self.api, is_subscribe_success, TIMEOUT)
            .await
            .expect("Expected SubscribeResponse for directory");
    }

    /// Wait for an UpdateNotification and parse it as a StorefrontState.
    pub async fn recv_storefront_update(&mut self) -> StorefrontState {
        let notif = recv_matching(&mut self.api, is_update_notification, TIMEOUT)
            .await
            .expect("Expected UpdateNotification for storefront");

        let bytes = extract_notification_bytes(&notif).expect("notification bytes");
        serde_json::from_slice(&bytes).expect("deserialize storefront from notification")
    }

    /// Wait for an UpdateNotification and parse it as a DirectoryState.
    pub async fn recv_directory_update(&mut self) -> DirectoryState {
        let notif = recv_matching(&mut self.api, is_update_notification, TIMEOUT)
            .await
            .expect("Expected UpdateNotification for directory");

        let bytes = extract_notification_bytes(&notif).expect("notification bytes");
        serde_json::from_slice(&bytes).expect("deserialize directory from notification")
    }
}

/// Top-level test fixture with named participants and a shared directory.
pub struct TestHarness {
    pub gary: Supplier,
    pub emma: Supplier,
    pub iris: Supplier,
    pub alice: Customer,
    pub bob: Customer,
    pub directory_key: ContractKey,
}

impl TestHarness {
    /// Set up the full harness: 5 WebSocket connections, identities, directory contract,
    /// 3 storefront contracts, and 3 directory registrations.
    pub async fn setup() -> Self {
        tracing_subscriber::fmt::try_init().ok();

        // Connect all 5 clients
        let api_gary = connect_to_node().await;
        let api_emma = connect_to_node().await;
        let api_iris = connect_to_node().await;
        let api_alice = connect_to_node().await;
        let api_bob = connect_to_node().await;

        // Create supplier identities (deterministic — same keys the UI derives)
        let (gary_id, gary_vk) = make_dummy_supplier("Gary");
        let (emma_id, emma_vk) = make_dummy_supplier("Emma");
        let (iris_id, iris_vk) = make_dummy_supplier("Iris");

        // Create customer identities (deterministic)
        let (alice_id, alice_vk) = make_dummy_customer("Alice");
        let (bob_id, bob_vk) = make_dummy_customer("Bob");

        // Create storefront contracts
        let (gary_sf_contract, gary_sf_key) = make_storefront_contract(&gary_vk);
        let (emma_sf_contract, emma_sf_key) = make_storefront_contract(&emma_vk);
        let (iris_sf_contract, iris_sf_key) = make_storefront_contract(&iris_vk);

        // Create directory contract
        let (dir_contract, dir_key) = make_directory_contract();

        // Build initial storefront states
        let gary_sf = make_initial_storefront(&gary_id, "Gary's Farm");
        let emma_sf = make_initial_storefront(&emma_id, "Emma's Farm");
        let iris_sf = make_initial_storefront(&iris_id, "Iris's Farm");

        // Assemble suppliers (need mutable for setup, so build partially)
        let mut gary = Supplier {
            name: "Gary".to_string(),
            id: gary_id.clone(),
            verifying_key: gary_vk,
            api: api_gary,
            storefront_key: gary_sf_key,
            storefront: gary_sf,
        };
        let mut emma = Supplier {
            name: "Emma".to_string(),
            id: emma_id.clone(),
            verifying_key: emma_vk,
            api: api_emma,
            storefront_key: emma_sf_key,
            storefront: emma_sf,
        };
        let mut iris = Supplier {
            name: "Iris".to_string(),
            id: iris_id.clone(),
            verifying_key: iris_vk,
            api: api_iris,
            storefront_key: iris_sf_key,
            storefront: iris_sf,
        };

        let alice = Customer {
            name: "Alice".to_string(),
            id: alice_id,
            verifying_key: alice_vk,
            api: api_alice,
        };
        let bob = Customer {
            name: "Bob".to_string(),
            id: bob_id,
            verifying_key: bob_vk,
            api: api_bob,
        };

        // PUT directory contract (via Gary's connection).
        // The directory key is deterministic (same WASM + empty params), so a parallel
        // test may have already created it. A short timeout handles this gracefully.
        let empty_dir = DirectoryState::default();
        let dir_state_bytes = serde_json::to_vec(&empty_dir).unwrap();
        gary.api
            .send(ClientRequest::ContractOp(ContractRequest::Put {
                contract: dir_contract,
                state: WrappedState::new(dir_state_bytes),
                related_contracts: RelatedContracts::default(),
                subscribe: false,
                blocking_subscribe: false,
            }))
            .await
            .unwrap();
        let dir_put = recv_matching(&mut gary.api, is_put_response, Duration::from_secs(2)).await;
        if dir_put.is_none() {
            tracing::info!("Directory contract already exists (parallel test), continuing");
        }

        // PUT all 3 storefront contracts
        put_storefront(&mut gary, gary_sf_contract).await;
        put_storefront(&mut emma, emma_sf_contract).await;
        put_storefront(&mut iris, iris_sf_contract).await;

        // Register all 3 suppliers in the directory
        register_supplier_in_directory(&mut gary, &dir_key).await;
        register_supplier_in_directory(&mut emma, &dir_key).await;
        register_supplier_in_directory(&mut iris, &dir_key).await;

        TestHarness {
            gary,
            emma,
            iris,
            alice,
            bob,
            directory_key: dir_key,
        }
    }
}

/// Build an empty StorefrontState for a supplier.
fn make_initial_storefront(owner: &SupplierId, name: &str) -> StorefrontState {
    StorefrontState {
        info: StorefrontInfo {
            owner: owner.clone(),
            name: name.to_string(),
            description: format!("{name} — fresh dairy direct"),
            location: GeoLocation::new(-33.87, 151.21),
        },
        products: BTreeMap::new(),
        orders: BTreeMap::new(),
    }
}

/// PUT a storefront contract via the supplier's connection and wait for confirmation.
async fn put_storefront(supplier: &mut Supplier, contract: ContractContainer) {
    let state_bytes = serde_json::to_vec(&supplier.storefront).unwrap();
    supplier
        .api
        .send(ClientRequest::ContractOp(ContractRequest::Put {
            contract,
            state: WrappedState::new(state_bytes),
            related_contracts: RelatedContracts::default(),
            subscribe: false,
            blocking_subscribe: false,
        }))
        .await
        .unwrap();

    recv_matching(&mut supplier.api, is_put_response, TIMEOUT)
        .await
        .unwrap_or_else(|| panic!("PutResponse for {}'s storefront", supplier.name));
}

/// Register a supplier in the directory via Update.
async fn register_supplier_in_directory(supplier: &mut Supplier, dir_key: &ContractKey) {
    let entry = make_directory_entry(&supplier.id, &supplier.name, supplier.storefront_key);
    let mut entries = BTreeMap::new();
    entries.insert(supplier.id.clone(), entry);
    let delta = DirectoryState { entries };
    let delta_bytes = serde_json::to_vec(&delta).unwrap();

    supplier
        .api
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: *dir_key,
            data: UpdateData::Delta(StateDelta::from(delta_bytes)),
        }))
        .await
        .unwrap();

    recv_matching(&mut supplier.api, is_update_response, TIMEOUT)
        .await
        .unwrap_or_else(|| {
            panic!(
                "UpdateResponse for {}'s directory registration",
                supplier.name
            )
        });
}
