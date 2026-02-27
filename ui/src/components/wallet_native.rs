//! CREAM-native wallet backend.
//!
//! Wraps the existing double-entry ledger (`record_transfer`) behind the
//! `WalletBackend` trait. All fund flows still go through user contracts
//! on the Freenet network — this is purely an abstraction layer.

use cream_common::wallet_backend::{TransferReceipt, WalletBackend, WalletError};
use dioxus::prelude::*;
use freenet_stdlib::prelude::ContractKey;

use super::node_api::{generate_tx_ref, now_iso8601, record_transfer, ContractRole};
use super::shared_state::SharedState;
use super::signing_service::SigningService;

/// CREAM-native wallet backed by on-network double-entry user contracts.
///
/// Holds only Copy types (Dioxus signals, contract keys) plus a signing service.
/// The `WebApi` handle is passed into each operation via `with_api()` since
/// it's borrowed mutably by the broader action handler and can't live inside
/// the wallet struct.
pub struct CreamNativeWallet {
    pub shared: Signal<SharedState>,
    pub root_contract_key: ContractKey,
    pub user_contract_key: Option<ContractKey>,
    pub signing_service: SigningService,
}

impl CreamNativeWallet {
    pub fn new(
        shared: Signal<SharedState>,
        root_contract_key: ContractKey,
        user_contract_key: Option<ContractKey>,
        signing_service: SigningService,
    ) -> Self {
        Self {
            shared,
            root_contract_key,
            user_contract_key,
            signing_service,
        }
    }

    /// Execute a transfer using the provided WebApi handle.
    ///
    /// This is the core method — trait methods call through here.
    pub async fn do_transfer(
        &mut self,
        api: &mut freenet_stdlib::client_api::WebApi,
        sender: ContractRole,
        receiver: ContractRole,
        amount: u64,
        description: String,
        sender_name: String,
        receiver_name: String,
    ) -> TransferReceipt {
        let tx_ref = generate_tx_ref(&sender_name);
        let timestamp = now_iso8601();

        record_transfer(
            api,
            &mut self.shared,
            sender,
            receiver,
            &self.root_contract_key,
            self.user_contract_key.as_ref(),
            amount,
            description,
            sender_name,
            receiver_name,
            None,
            &self.signing_service,
        )
        .await;

        TransferReceipt {
            tx_ref,
            amount,
            timestamp,
            bearer_token: None, // CREAM-native has no bearer tokens
        }
    }

    /// Execute a transfer with a caller-supplied tx_ref (for idempotency).
    pub async fn do_transfer_with_ref(
        &mut self,
        api: &mut freenet_stdlib::client_api::WebApi,
        sender: ContractRole,
        receiver: ContractRole,
        amount: u64,
        description: String,
        sender_name: String,
        receiver_name: String,
        tx_ref: String,
    ) -> TransferReceipt {
        let timestamp = now_iso8601();

        record_transfer(
            api,
            &mut self.shared,
            sender,
            receiver,
            &self.root_contract_key,
            self.user_contract_key.as_ref(),
            amount,
            description,
            sender_name,
            receiver_name,
            Some(tx_ref.clone()),
            &self.signing_service,
        )
        .await;

        TransferReceipt {
            tx_ref,
            amount,
            timestamp,
            bearer_token: None,
        }
    }

    /// Transfer from root to user (e.g. registration bonus, faucet, escrow release).
    pub async fn transfer_from_root(
        &mut self,
        api: &mut freenet_stdlib::client_api::WebApi,
        amount: u64,
        description: String,
        recipient_name: String,
    ) -> TransferReceipt {
        self.do_transfer(
            api,
            ContractRole::Root,
            ContractRole::User,
            amount,
            description,
            cream_common::identity::ROOT_USER_NAME.to_string(),
            recipient_name,
        )
        .await
    }

    /// Transfer from root to user with a deterministic tx_ref for idempotency.
    /// Used for initial CURD allocation so re-registration doesn't double-allocate.
    pub async fn transfer_from_root_idempotent(
        &mut self,
        api: &mut freenet_stdlib::client_api::WebApi,
        amount: u64,
        description: String,
        recipient_name: String,
        tx_ref: String,
    ) -> TransferReceipt {
        self.do_transfer_with_ref(
            api,
            ContractRole::Root,
            ContractRole::User,
            amount,
            description,
            cream_common::identity::ROOT_USER_NAME.to_string(),
            recipient_name,
            tx_ref,
        )
        .await
    }

    /// Transfer from root to a third-party contract (e.g. supplier registration).
    pub async fn transfer_from_root_to_third_party(
        &mut self,
        api: &mut freenet_stdlib::client_api::WebApi,
        recipient_key: ContractKey,
        amount: u64,
        description: String,
        recipient_name: String,
    ) -> TransferReceipt {
        self.do_transfer(
            api,
            ContractRole::Root,
            ContractRole::ThirdParty(recipient_key),
            amount,
            description,
            cream_common::identity::ROOT_USER_NAME.to_string(),
            recipient_name,
        )
        .await
    }

    /// Transfer from root to third-party with a deterministic tx_ref for idempotency.
    pub async fn transfer_from_root_to_third_party_idempotent(
        &mut self,
        api: &mut freenet_stdlib::client_api::WebApi,
        recipient_key: ContractKey,
        amount: u64,
        description: String,
        recipient_name: String,
        tx_ref: String,
    ) -> TransferReceipt {
        self.do_transfer_with_ref(
            api,
            ContractRole::Root,
            ContractRole::ThirdParty(recipient_key),
            amount,
            description,
            cream_common::identity::ROOT_USER_NAME.to_string(),
            recipient_name,
            tx_ref,
        )
        .await
    }

    /// Transfer from user to root (e.g. order deposit, message toll).
    pub async fn transfer_to_root(
        &mut self,
        api: &mut freenet_stdlib::client_api::WebApi,
        amount: u64,
        description: String,
        sender_name: String,
    ) -> TransferReceipt {
        self.do_transfer(
            api,
            ContractRole::User,
            ContractRole::Root,
            amount,
            description,
            sender_name,
            cream_common::identity::ROOT_USER_NAME.to_string(),
        )
        .await
    }

    /// Transfer from root to a supplier's user contract (e.g. escrow settlement).
    ///
    /// Temporarily overrides the wallet's user_contract_key with the supplier's
    /// key for the duration of this transfer, then restores it.
    pub async fn settle_escrow_to_supplier(
        &mut self,
        api: &mut freenet_stdlib::client_api::WebApi,
        supplier_uc_key: ContractKey,
        amount: u64,
        description: String,
        supplier_name: String,
    ) -> TransferReceipt {
        let saved_key = self.user_contract_key;
        self.user_contract_key = Some(supplier_uc_key);
        let receipt = self
            .do_transfer(
                api,
                ContractRole::Root,
                ContractRole::User,
                amount,
                description,
                cream_common::identity::ROOT_USER_NAME.to_string(),
                supplier_name,
            )
            .await;
        self.user_contract_key = saved_key;
        receipt
    }
}

impl WalletBackend for CreamNativeWallet {
    async fn balance(&self) -> Result<u64, WalletError> {
        let state = self.shared.read();
        if let Some(ref uc) = state.user_contract {
            Ok(uc.balance_curds)
        } else {
            Err(WalletError::BackendUnavailable(
                "user contract not loaded".to_string(),
            ))
        }
    }

    async fn transfer(
        &mut self,
        _amount: u64,
        _description: String,
        _recipient: String,
    ) -> Result<TransferReceipt, WalletError> {
        // Trait method can't accept &mut WebApi. For CREAM-native, callers use
        // the typed helpers (transfer_from_root, transfer_to_root, etc.) which
        // accept the api handle. This trait method exists for the Fedimint
        // backend which owns its own connection.
        Err(WalletError::TransferFailed(
            "use typed transfer helpers for CREAM-native backend".to_string(),
        ))
    }

    async fn receive(&mut self, _token: &str) -> Result<u64, WalletError> {
        // CREAM-native doesn't use bearer tokens — credits arrive via record_transfer
        Err(WalletError::TransferFailed(
            "CREAM-native uses double-entry ledger, not bearer tokens".to_string(),
        ))
    }

    async fn escrow_lock(
        &mut self,
        _amount: u64,
        _description: String,
    ) -> Result<String, WalletError> {
        // For CREAM-native, escrow is implicit via root account. The "token"
        // is just the tx_ref. Actual transfer done via transfer_to_root().
        Err(WalletError::TransferFailed(
            "use transfer_to_root for CREAM-native escrow lock".to_string(),
        ))
    }

    async fn escrow_release(
        &mut self,
        _token: &str,
        _recipient: String,
    ) -> Result<TransferReceipt, WalletError> {
        Err(WalletError::TransferFailed(
            "use settle_escrow_to_supplier for CREAM-native escrow release".to_string(),
        ))
    }

    async fn escrow_cancel(&mut self, _token: &str) -> Result<u64, WalletError> {
        // For CREAM-native, cancel escrow is a transfer from root → user
        Err(WalletError::TransferFailed(
            "use transfer_from_root for CREAM-native escrow cancel".to_string(),
        ))
    }

    fn backend_name(&self) -> &str {
        "cream-native"
    }
}
