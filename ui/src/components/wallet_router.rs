//! Wallet router for parallel validation of CREAM-native vs Fedimint backends.
//!
//! In `Parallel` mode, every operation executes on both backends and compares
//! results, logging discrepancies. This validates CREAM-native correctness
//! against Fedimint as a reference implementation.

// TODO: Enable Parallel/Fedimint modes when Fedimint backend is available.
//
// pub enum WalletMode {
//     /// CREAM-native double-entry ledger only (current production path).
//     Native,
//     /// Fedimint e-cash only.
//     Fedimint,
//     /// Both backends execute every operation; discrepancies are logged.
//     Parallel,
// }
//
// pub struct WalletRouter {
//     native: CreamNativeWallet,
//     fedimint: Option<FedimintWallet>,
//     mode: WalletMode,
// }
//
// impl WalletRouter {
//     pub fn native_only(wallet: CreamNativeWallet) -> Self {
//         Self { native: wallet, fedimint: None, mode: WalletMode::Native }
//     }
//
//     pub fn parallel(native: CreamNativeWallet, fedimint: FedimintWallet) -> Self {
//         Self { native, fedimint: Some(fedimint), mode: WalletMode::Parallel }
//     }
// }
//
// In Parallel mode, each operation:
// 1. Executes on CREAM-native (source of truth, result returned to caller)
// 2. Executes on Fedimint
// 3. Compares balances, logs discrepancies to console
