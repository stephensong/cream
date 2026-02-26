//! Fedimint e-cash wallet backend.
//!
//! Implements `WalletBackend` using Fedimint's e-cash system. This backend
//! owns its own connection to a Fedimint federation and doesn't need the
//! Freenet WebApi handle.
//!
//! Build requirements:
//! - Feature flag `fedimint` must be enabled in ui/Cargo.toml
//! - Depends on `fedimint-client`, `fedimint-mint-client`, `fedimint-core`
//!
//! WASM strategy: Uses `fedimint-client-rpc` JSON-RPC bridge loaded as a
//! separate Web Worker WASM module. CREAM UI communicates via `postMessage`.
//! This avoids polluting CREAM's WASM build with Fedimint's dependency tree.

// TODO: Enable when Fedimint deps are available (requires new hardware for build)
//
// Key mappings from WalletBackend → Fedimint SDK:
//
//   balance()        → client.get_balance_for_btc()
//   transfer()       → mint.spend_notes_with_selector() → return OOBNotes string
//   receive()        → mint.reissue_external_notes(parsed_notes)
//   escrow_lock()    → mint.spend_notes_with_selector() → return OOBNotes as token
//   escrow_release() → recipient calls mint.reissue_external_notes(token)
//   escrow_cancel()  → mint.try_cancel_spend_notes(operation_id)
//
// use cream_common::wallet_backend::{TransferReceipt, WalletBackend, WalletError};
//
// pub struct FedimintWallet {
//     // For native tests: fedimint_client::ClientHandleArc
//     // For WASM: JSON-RPC channel to Web Worker
// }
//
// impl WalletBackend for FedimintWallet {
//     async fn balance(&self) -> Result<u64, WalletError> { ... }
//     async fn transfer(&mut self, amount: u64, description: String, recipient: String)
//         -> Result<TransferReceipt, WalletError> { ... }
//     async fn receive(&mut self, token: &str) -> Result<u64, WalletError> { ... }
//     async fn escrow_lock(&mut self, amount: u64, description: String)
//         -> Result<String, WalletError> { ... }
//     async fn escrow_release(&mut self, token: &str, recipient: String)
//         -> Result<TransferReceipt, WalletError> { ... }
//     async fn escrow_cancel(&mut self, token: &str) -> Result<u64, WalletError> { ... }
//     fn backend_name(&self) -> &str { "fedimint" }
// }
