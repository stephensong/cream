//! Standalone CURD conservation invariant checker.
//!
//! Connects to a Freenet node, dynamically discovers all user contracts from
//! the system root's ledger, and verifies:
//! 1. Each contract's `balance_curds` == `derive_balance()` (cache matches ledger)
//! 2. Sum of all balances == SYSTEM_FLOAT (1,000,000 CURD)
//!
//! Exits 0 on success, 1 on failure. Output goes to stdout.
//!
//! Usage:
//!   check-invariants [LABEL] [--port PORT]
//!
//! LABEL is an optional human-readable context string (e.g. "e2e test 09 pre").
//! --port: Freenet node WebSocket port (default: 3001, the gateway).

use cream_node_integration::harness::{check_curd_conservation, user_contract_key_for};
use cream_node_integration::{connect_to_node_at, node_url, wait_for_get};
use cream_common::user_contract::UserContractState;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let port = args
        .windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse::<u16>().ok())
        .unwrap_or(3001);

    let dump = args.iter().any(|a| a == "--dump");

    if dump {
        // Dump mode: print ledger entries for a named contract
        let name = args.get(1).filter(|a| *a != "--dump" && *a != "--port")
            .cloned().unwrap_or("system_root".to_string());
        let key = user_contract_key_for(&name);
        let url = node_url(port);
        let mut api = connect_to_node_at(&url).await;
        let bytes = wait_for_get(&mut api, *key.id(), Duration::from_secs(30)).await
            .expect("GET contract");
        let state: UserContractState = serde_json::from_slice(&bytes).unwrap();
        println!("Contract: {} (port {})", name, port);
        println!("balance_curds: {}", state.balance_curds);
        println!("derive_balance(): {}", state.derive_balance());
        println!("checkpoint_balance: {}", state.checkpoint_balance);
        println!("Ledger ({} entries):", state.ledger.len());
        for tx in &state.ledger {
            println!("  {:?} {:>6} {:30} sender={:20} receiver={:20} tx_ref={}",
                tx.kind, tx.amount, tx.description, tx.sender, tx.receiver, tx.tx_ref);
        }
        return;
    }

    let label = args
        .get(1)
        .filter(|a| *a != "--port")
        .cloned()
        .unwrap_or_else(|| "standalone".to_string());

    check_curd_conservation(&label, port).await;
}
