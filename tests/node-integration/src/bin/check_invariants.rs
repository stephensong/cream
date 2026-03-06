//! Standalone CURD conservation invariant checker.
//!
//! Connects to a Freenet node, GETs all user contracts, and verifies:
//! 1. Each contract's `balance_curds` == `derive_balance()` (cache matches ledger)
//! 2. Sum of all balances == SYSTEM_FLOAT (1,000,000 CURD) — soft check in E2E mode
//!
//! Exits 0 on success, 1 on failure. Output goes to stdout.
//!
//! Usage:
//!   check-invariants [LABEL] [--soft]
//!
//! LABEL is an optional human-readable context string (e.g. "e2e test 09 pre").
//! --soft: only assert cache/ledger consistency, warn (don't fail) on total mismatch.
//!         Used by E2E tests where the UI may create additional transfers that affect
//!         the total but don't indicate bugs.

use cream_node_integration::harness::check_curd_conservation;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let soft = args.iter().any(|a| a == "--soft");
    let label = args
        .get(1)
        .filter(|a| *a != "--soft")
        .cloned()
        .unwrap_or_else(|| "standalone".to_string());

    check_curd_conservation(&label, soft).await;
}
