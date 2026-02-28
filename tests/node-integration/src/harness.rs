use std::collections::BTreeMap;
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use freenet_stdlib::client_api::{ClientRequest, ContractRequest, WebApi};
use freenet_stdlib::prelude::*;

use cream_common::directory::DirectoryState;
use cream_common::identity::{CustomerId, SupplierId};
use cream_common::location::GeoLocation;
use cream_common::order::Order;
use cream_common::product::{Product, ProductCategory, ProductId};
use cream_common::storefront::{SignedProduct, StorefrontInfo, StorefrontState, WeeklySchedule};

use cream_common::user_contract::UserContractState;
use cream_common::wallet::{TransactionKind, WalletTransaction};

use crate::{
    connect_to_node_at, extract_get_response_state, extract_notification_bytes, is_get_response,
    is_put_response, is_subscribe_success, is_update_notification, is_update_response,
    make_directory_contract, make_directory_entry, make_dummy_customer, make_dummy_supplier,
    make_storefront_contract, make_user_contract, node_url, recv_matching, wait_for_get,
    wait_for_put,
};

const TIMEOUT: Duration = Duration::from_secs(60);

/// A supplier participant in the test harness.
pub struct Supplier {
    pub name: String,
    pub id: SupplierId,
    pub verifying_key: VerifyingKey,
    pub api: WebApi,
    pub storefront_key: ContractKey,
    pub storefront: StorefrontState,
    pub postcode: String,
    pub locality: String,
    /// Contract key for this supplier's user contract.
    pub user_contract_key: Option<ContractKey>,
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
                quantity_total: 10,
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

    /// Add a pre-built order to this supplier's storefront, send the update, and wait for confirmation.
    pub async fn add_order(&mut self, order: Order) {
        let order_id = order.id.clone();
        self.storefront.orders.insert(order_id, order);

        let sf_bytes = serde_json::to_vec(&self.storefront).unwrap();
        self.api
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: self.storefront_key,
                data: UpdateData::State(State::from(sf_bytes)),
            }))
            .await
            .unwrap();

        recv_matching(&mut self.api, is_update_response, TIMEOUT)
            .await
            .expect("Expected UpdateResponse after adding order");
    }

    /// Run expire_orders on the local storefront, push update if any changed.
    /// Returns true if any orders were expired.
    pub async fn expire_orders(&mut self) -> bool {
        let now = chrono::Utc::now();
        if self.storefront.expire_orders(now) {
            let sf_bytes = serde_json::to_vec(&self.storefront).unwrap();
            self.api
                .send(ClientRequest::ContractOp(ContractRequest::Update {
                    key: self.storefront_key,
                    data: UpdateData::State(State::from(sf_bytes)),
                }))
                .await
                .unwrap();

            recv_matching(&mut self.api, is_update_response, TIMEOUT)
                .await
                .expect("Expected UpdateResponse after expiring orders");
            true
        } else {
            false
        }
    }

    /// Update the supplier's opening hours schedule and push to the network.
    pub async fn update_schedule(&mut self, schedule: WeeklySchedule, timezone: &str) {
        self.storefront.info.schedule = Some(schedule);
        self.storefront.info.timezone = Some(timezone.to_string());

        let sf_bytes = serde_json::to_vec(&self.storefront).unwrap();
        self.api
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: self.storefront_key,
                data: UpdateData::State(State::from(sf_bytes)),
            }))
            .await
            .unwrap();

        recv_matching(&mut self.api, is_update_response, TIMEOUT)
            .await
            .unwrap_or_else(|| panic!("UpdateResponse for {}'s schedule", self.name));
    }

    /// Fulfill an order on this supplier's storefront (Reserved/Paid → Fulfilled).
    pub async fn fulfill_order(&mut self, order_id: &str) {
        use cream_common::order::OrderStatus;

        let oid = cream_common::order::OrderId(order_id.to_string());
        let order = self
            .storefront
            .orders
            .get_mut(&oid)
            .unwrap_or_else(|| panic!("Order {} not found on {}'s storefront", order_id, self.name));
        assert!(
            order.status.can_transition_to(&OrderStatus::Fulfilled),
            "Cannot fulfill order {} in status {}",
            order_id,
            order.status
        );
        order.status = OrderStatus::Fulfilled;

        let sf_bytes = serde_json::to_vec(&self.storefront).unwrap();
        self.api
            .send(ClientRequest::ContractOp(ContractRequest::Update {
                key: self.storefront_key,
                data: UpdateData::State(State::from(sf_bytes)),
            }))
            .await
            .unwrap();

        recv_matching(&mut self.api, is_update_response, TIMEOUT)
            .await
            .expect("Expected UpdateResponse after fulfilling order");
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
    /// CURD wallet balance (derived from on-network ledger).
    pub balance: u64,
    /// Contract key for this customer's user contract.
    pub user_contract_key: Option<ContractKey>,
}

impl Customer {
    /// Place an order if balance is sufficient. Decrements balance and pushes the order
    /// to the supplier's storefront. Returns `Err` if the customer can't afford the deposit.
    pub async fn place_order(
        &mut self,
        order: Order,
        supplier: &mut Supplier,
    ) -> Result<(), String> {
        let deposit = order.deposit_amount;
        if self.balance < deposit {
            return Err(format!(
                "Insufficient balance: have {}, need {} deposit",
                self.balance, deposit
            ));
        }
        self.balance -= deposit;
        supplier.add_order(order).await;
        Ok(())
    }

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
    pub root_contract_key: ContractKey,
}

impl TestHarness {
    /// Set up the full harness: 5 WebSocket connections, identities, directory contract,
    /// 3 storefront contracts, and 3 directory registrations.
    ///
    /// Participants are distributed across nodes:
    /// - **Gateway (3001)**: Gary, Iris, Alice — Gary PUTs contracts here
    /// - **Node 2 (3003)**: Emma, Bob — exercises cross-node propagation
    pub async fn setup() -> Self {
        tracing_subscriber::fmt::try_init().ok();

        let url_gw = node_url(3001);
        let url_n2 = node_url(3003);

        // Connect clients — gateway for Gary/Iris/Alice, node-2 for Emma/Bob
        let api_gary = connect_to_node_at(&url_gw).await;
        let api_emma = connect_to_node_at(&url_n2).await;
        let api_iris = connect_to_node_at(&url_gw).await;
        let api_alice = connect_to_node_at(&url_gw).await;
        let api_bob = connect_to_node_at(&url_n2).await;

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
        let mut gary_sf = make_initial_storefront(
            &gary_id,
            "Gary's Farm",
            "Real Beaut Dairy",
            GeoLocation::new(-30.0977, 152.6583),
        );
        // Populate Gary's schedule: Mon–Fri 9:00–17:00, Sat 9:00–12:00
        // (Step 7 will update this to Mon–Fri 8:00–17:00 to test notifications)
        let mut schedule = WeeklySchedule::new();
        for day in 0..5u8 {
            schedule.set_range(day, 18, 34, true); // 9:00 = slot 18, 17:00 = slot 34
        }
        schedule.set_range(5, 18, 24, true); // Sat: 9:00 = slot 18, 12:00 = slot 24
        gary_sf.info.schedule = Some(schedule);
        gary_sf.info.timezone = Some("Australia/Sydney".to_string());
        let emma_sf = make_initial_storefront(
            &emma_id,
            "Emma's Farm",
            "Emma's Dairy",
            GeoLocation::new(-30.0977, 152.6583),
        );
        let iris_sf = make_initial_storefront(
            &iris_id,
            "Iris's Farm",
            "Iris's farm — fresh dairy direct",
            GeoLocation::new(-33.87, 151.21),
        );

        // Assemble suppliers (need mutable for setup, so build partially)
        let mut gary = Supplier {
            name: "Gary".to_string(),
            id: gary_id.clone(),
            verifying_key: gary_vk,
            api: api_gary,
            storefront_key: gary_sf_key,
            storefront: gary_sf,
            postcode: "2450".to_string(),
            locality: "Boambee".to_string(),
            user_contract_key: None,
        };
        let mut emma = Supplier {
            name: "Emma".to_string(),
            id: emma_id.clone(),
            verifying_key: emma_vk,
            api: api_emma,
            storefront_key: emma_sf_key,
            storefront: emma_sf,
            postcode: "2450".to_string(),
            locality: "Boambee".to_string(),
            user_contract_key: None,
        };
        let mut iris = Supplier {
            name: "Iris".to_string(),
            id: iris_id.clone(),
            verifying_key: iris_vk,
            api: api_iris,
            storefront_key: iris_sf_key,
            storefront: iris_sf,
            postcode: "2000".to_string(),
            locality: "Sydney".to_string(),
            user_contract_key: None,
        };

        let mut alice = Customer {
            name: "Alice".to_string(),
            id: alice_id,
            verifying_key: alice_vk,
            api: api_alice,
            balance: 10_000,
            user_contract_key: None,
        };
        let mut bob = Customer {
            name: "Bob".to_string(),
            id: bob_id,
            verifying_key: bob_vk,
            api: api_bob,
            balance: 10_000,
            user_contract_key: None,
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

        // PUT storefront contracts on their respective nodes.
        // Gary and Iris PUT on gateway; Emma PUTs on node-2.
        put_storefront(&mut gary, gary_sf_contract).await;
        put_storefront(&mut iris, iris_sf_contract).await;

        // Wait for directory contract to propagate to node-2 before Emma PUTs
        let mut emma_dir_probe = connect_to_node_at(&url_n2).await;
        wait_for_get(&mut emma_dir_probe, *dir_key.id(), TIMEOUT)
            .await
            .expect("Directory contract should propagate to node-2");
        drop(emma_dir_probe);

        put_storefront(&mut emma, emma_sf_contract).await;

        // Deploy root user contract with 1,000,000 CURD genesis credit.
        let root_id = cream_common::identity::root_customer_id();
        let root_vk = *root_id.0.as_bytes();
        let root_vk = ed25519_dalek::VerifyingKey::from_bytes(&root_vk).unwrap();
        let (root_contract, root_key) = make_user_contract(&root_vk);

        let genesis_tx = WalletTransaction {
            id: 0,
            kind: TransactionKind::Credit,
            amount: 1_000_000,
            description: "Genesis".to_string(),
            sender: String::new(),
            receiver: cream_common::identity::ROOT_USER_NAME.to_string(),
            tx_ref: "genesis:0:0".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            lightning_payment_hash: None,
        };

        let mut root_state = UserContractState {
            owner: cream_common::identity::root_customer_id(),
            name: cream_common::identity::ROOT_USER_NAME.to_string(),
            origin_supplier: String::new(),
            current_supplier: String::new(),
            balance_curds: 1_000_000,
            invited_by: String::new(),
            ledger: vec![genesis_tx],
            next_tx_id: 1,
            updated_at: chrono::Utc::now(),
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
        };
        root_state.signature = cream_common::identity::root_sign(&root_state.signable_bytes());
        let root_state_bytes = serde_json::to_vec(&root_state).unwrap();

        // Deploy root contract on gateway (via Gary's connection, reuse a fresh one)
        let mut root_api = connect_to_node_at(&url_gw).await;
        wait_for_put(
            &mut root_api,
            root_contract,
            WrappedState::new(root_state_bytes),
            TIMEOUT,
        )
        .await
        .expect("PutResponse for root user contract");
        drop(root_api);

        // Deploy user contracts for Alice and Bob with initial 10,000 CURD allocation.
        deploy_user_contract(&mut alice, &root_key, &url_gw, "Gary").await;
        deploy_user_contract(&mut bob, &root_key, &url_n2, "Gary").await;

        // Deploy user contracts for suppliers (Gary, Emma, Iris) with 10,000 CURD each.
        // Must happen before directory registration so user_contract_key is included.
        deploy_supplier_user_contract(&mut gary, &root_key, &url_gw).await;
        deploy_supplier_user_contract(&mut emma, &root_key, &url_n2).await;
        deploy_supplier_user_contract(&mut iris, &root_key, &url_gw).await;

        // Register all 3 suppliers in the directory (after user contracts so keys are available).
        // Gary and Iris register via gateway; Emma registers via node-2.
        register_supplier_in_directory(&mut gary, &dir_key).await;
        register_supplier_in_directory(&mut iris, &dir_key).await;
        register_supplier_in_directory(&mut emma, &dir_key).await;

        TestHarness {
            gary,
            emma,
            iris,
            alice,
            bob,
            directory_key: dir_key,
            root_contract_key: root_key,
        }
    }
}

/// Build an empty StorefrontState for a supplier.
fn make_initial_storefront(
    owner: &SupplierId,
    name: &str,
    description: &str,
    location: GeoLocation,
) -> StorefrontState {
    StorefrontState {
        info: StorefrontInfo {
            owner: owner.clone(),
            name: name.to_string(),
            description: description.to_string(),
            location,
            schedule: None,
            timezone: None,
            phone: None,
            email: None,
            address: None,
        },
        products: BTreeMap::new(),
        orders: BTreeMap::new(),
        messages: BTreeMap::new(),
    }
}

/// PUT a storefront contract via the supplier's connection and wait for confirmation.
async fn put_storefront(supplier: &mut Supplier, contract: ContractContainer) {
    let state_bytes = serde_json::to_vec(&supplier.storefront).unwrap();
    wait_for_put(
        &mut supplier.api,
        contract,
        WrappedState::new(state_bytes),
        TIMEOUT,
    )
    .await
    .unwrap_or_else(|| panic!("PutResponse for {}'s storefront", supplier.name));
}

/// Register a supplier in the directory via Update.
async fn register_supplier_in_directory(supplier: &mut Supplier, dir_key: &ContractKey) {
    let entry = make_directory_entry(
        &supplier.id,
        &supplier.name,
        &supplier.storefront.info.description,
        &supplier.postcode,
        &supplier.locality,
        supplier.storefront.info.location.clone(),
        supplier.storefront_key,
        supplier.user_contract_key,
    );
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

/// Deploy a user contract for a supplier with initial 10,000 CURD from root.
async fn deploy_supplier_user_contract(
    supplier: &mut Supplier,
    root_key: &ContractKey,
    node_url: &str,
) {
    let (uc_contract, uc_key) = make_user_contract(&supplier.verifying_key);

    // Deterministic tx_ref so UI re-registration deduplicates against this credit.
    let tx_ref = format!("genesis:{}", supplier.name);
    let now_str = chrono::Utc::now().to_rfc3339();

    let initial_credit = WalletTransaction {
        id: 0,
        kind: TransactionKind::Credit,
        amount: 10_000,
        description: "Initial CURD allocation".to_string(),
        sender: cream_common::identity::ROOT_USER_NAME.to_string(),
        receiver: supplier.name.clone(),
        tx_ref: tx_ref.clone(),
        timestamp: now_str.clone(),
        lightning_payment_hash: None,
    };

    let uc_state = UserContractState {
        owner: cream_common::identity::CustomerId(supplier.verifying_key),
        name: supplier.name.clone(),
        origin_supplier: supplier.name.clone(),
        current_supplier: supplier.name.clone(),
        balance_curds: 10_000,
        invited_by: String::new(),
        ledger: vec![initial_credit],
        next_tx_id: 1,
        updated_at: chrono::Utc::now(),
        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
    };

    let uc_state_bytes = serde_json::to_vec(&uc_state).unwrap();

    let mut deploy_api = connect_to_node_at(node_url).await;
    wait_for_put(
        &mut deploy_api,
        uc_contract,
        WrappedState::new(uc_state_bytes),
        TIMEOUT,
    )
    .await
    .unwrap_or_else(|| panic!("PutResponse for {}'s supplier user contract", supplier.name));
    drop(deploy_api);

    supplier.user_contract_key = Some(uc_key);

    // Also record the debit on root's contract
    let root_debit = WalletTransaction {
        id: 0,
        kind: TransactionKind::Debit,
        amount: 10_000,
        description: format!("Initial CURD allocation for {}", supplier.name),
        sender: cream_common::identity::ROOT_USER_NAME.to_string(),
        receiver: supplier.name.clone(),
        tx_ref,
        timestamp: now_str,
        lightning_payment_hash: None,
    };

    // GET root state, append debit, Update
    let mut root_api = connect_to_node_at(node_url).await;
    let root_bytes = wait_for_get(&mut root_api, *root_key.id(), TIMEOUT)
        .await
        .expect("GET root contract for supplier debit");
    let mut root_state: UserContractState = serde_json::from_slice(&root_bytes).unwrap();
    root_state.ledger.push(root_debit);
    root_state.balance_curds = root_state.derive_balance();
    root_state.next_tx_id = root_state.ledger.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    root_state.updated_at = chrono::Utc::now();
    root_state.signature = cream_common::identity::root_sign(&root_state.signable_bytes());

    let root_update_bytes = serde_json::to_vec(&root_state).unwrap();
    root_api
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: *root_key,
            data: UpdateData::State(State::from(root_update_bytes)),
        }))
        .await
        .unwrap();

    recv_matching(&mut root_api, is_update_response, TIMEOUT)
        .await
        .expect("UpdateResponse for root debit (supplier)");
    drop(root_api);
}

/// Deploy a user contract for a customer with initial 10,000 CURD from root.
async fn deploy_user_contract(
    customer: &mut Customer,
    root_key: &ContractKey,
    node_url: &str,
    invited_by: &str,
) {
    let (uc_contract, uc_key) = make_user_contract(&customer.verifying_key);

    // Deterministic tx_ref so UI re-registration deduplicates against this credit.
    let tx_ref = format!("genesis:{}", customer.name);
    let now_str = chrono::Utc::now().to_rfc3339();

    let initial_credit = WalletTransaction {
        id: 0,
        kind: TransactionKind::Credit,
        amount: 10_000,
        description: "Initial CURD allocation".to_string(),
        sender: cream_common::identity::ROOT_USER_NAME.to_string(),
        receiver: customer.name.clone(),
        tx_ref: tx_ref.clone(),
        timestamp: now_str.clone(),
        lightning_payment_hash: None,
    };

    let uc_state = UserContractState {
        owner: customer.id.clone(),
        name: customer.name.clone(),
        origin_supplier: invited_by.to_string(),
        current_supplier: invited_by.to_string(),
        balance_curds: 10_000,
        invited_by: invited_by.to_string(),
        ledger: vec![initial_credit],
        next_tx_id: 1,
        updated_at: chrono::Utc::now(),
        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
    };

    let uc_state_bytes = serde_json::to_vec(&uc_state).unwrap();

    let mut deploy_api = connect_to_node_at(node_url).await;
    wait_for_put(
        &mut deploy_api,
        uc_contract,
        WrappedState::new(uc_state_bytes),
        TIMEOUT,
    )
    .await
    .unwrap_or_else(|| panic!("PutResponse for {}'s user contract", customer.name));
    drop(deploy_api);

    customer.user_contract_key = Some(uc_key);

    // Also record the debit on root's contract
    let root_debit = WalletTransaction {
        id: 0,
        kind: TransactionKind::Debit,
        amount: 10_000,
        description: format!("Initial CURD allocation for {}", customer.name),
        sender: cream_common::identity::ROOT_USER_NAME.to_string(),
        receiver: customer.name.clone(),
        tx_ref,
        timestamp: now_str,
        lightning_payment_hash: None,
    };

    // GET root state, append debit, Update
    let mut root_api = connect_to_node_at(node_url).await;
    let root_bytes = wait_for_get(&mut root_api, *root_key.id(), TIMEOUT)
        .await
        .expect("GET root contract for debit");
    let mut root_state: UserContractState = serde_json::from_slice(&root_bytes).unwrap();
    root_state.ledger.push(root_debit);
    root_state.balance_curds = root_state.derive_balance();
    root_state.next_tx_id = root_state.ledger.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    root_state.updated_at = chrono::Utc::now();
    root_state.signature = cream_common::identity::root_sign(&root_state.signable_bytes());

    let root_update_bytes = serde_json::to_vec(&root_state).unwrap();
    root_api
        .send(ClientRequest::ContractOp(ContractRequest::Update {
            key: *root_key,
            data: UpdateData::State(State::from(root_update_bytes)),
        }))
        .await
        .unwrap();

    recv_matching(&mut root_api, is_update_response, TIMEOUT)
        .await
        .expect("UpdateResponse for root debit");
    drop(root_api);
}
