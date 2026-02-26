//! Standalone Fedimint e-cash integration tests.
//!
//! These tests exercise the Fedimint SDK operations that CREAM will need
//! for its wallet backend, using an in-process test federation with fake
//! Bitcoin. No CREAM code is touched — this purely validates that the SDK
//! works on this machine.

use std::time::Duration;

use anyhow::Result;
use fedimint_client::ClientHandleArc;
use fedimint_client::transaction::TransactionBuilder;
use fedimint_client_module::ClientModule;
use fedimint_core::core::OperationId;
use fedimint_core::util::NextOrPending;
use fedimint_core::{Amount, sats};
use fedimint_dummy_client::{DummyClientInit, DummyClientModule};
use fedimint_dummy_server::DummyInit;
use fedimint_mint_client::{
    MintClientInit, MintClientModule, ReissueExternalNotesState, SelectNotesWithAtleastAmount,
    SpendOOBState,
};
use fedimint_mint_server::MintInit;
use fedimint_testing::fixtures::Fixtures;
use futures::StreamExt;

const TIMEOUT: Duration = Duration::from_secs(15);

fn fixtures() -> Fixtures {
    Fixtures::new_primary(MintClientInit, MintInit).with_module(DummyClientInit, DummyInit)
}

/// Issue e-cash to a client via the dummy module (creates "free money").
async fn issue_ecash(client: &ClientHandleArc, amount: Amount) -> Result<()> {
    let dummy = client.get_first_module::<DummyClientModule>()?;
    let input = dummy.create_input(amount);
    let op_id = OperationId::new_random();
    let range = client
        .finalize_and_submit_transaction(
            op_id,
            "issue test ecash",
            |_| (),
            TransactionBuilder::new().with_inputs(input),
        )
        .await?;
    client
        .await_primary_bitcoin_module_outputs(op_id, range.into_iter().collect())
        .await?;
    Ok(())
}

/// Test 1: Spin up a 4-peer in-process federation and verify it runs.
#[tokio::test(flavor = "multi_thread")]
async fn test_federation_startup() -> Result<()> {
    let _fed = fixtures().new_fed_not_degraded().await;
    // If we get here without panicking, the federation started successfully.
    Ok(())
}

/// Test 2: Issue e-cash to a client and check the balance.
#[tokio::test(flavor = "multi_thread")]
async fn test_ecash_issue_and_balance() -> Result<()> {
    let fed = fixtures().new_fed_not_degraded().await;
    let client = fed.new_client().await;

    assert_eq!(client.get_balance_for_btc().await?, Amount::ZERO);

    issue_ecash(&client, sats(1000)).await?;

    let balance = client.get_balance_for_btc().await?;
    assert!(
        balance >= sats(1000),
        "Expected at least 1000 sats, got {balance}"
    );

    Ok(())
}

/// Test 3: Two clients — spend OOBNotes from one, reissue on the other.
#[tokio::test(flavor = "multi_thread")]
async fn test_ecash_spend_receive() -> Result<()> {
    let fed = fixtures().new_fed_not_degraded().await;
    let (sender, receiver) = fed.two_clients().await;

    issue_ecash(&sender, sats(1000)).await?;

    let sender_mint = sender.get_first_module::<MintClientModule>()?;
    let receiver_mint = receiver.get_first_module::<MintClientModule>()?;

    // Sender spends 750 sats as OOBNotes
    let (_spend_op, notes) = sender_mint
        .spend_notes_with_selector(&SelectNotesWithAtleastAmount, sats(750), TIMEOUT, false, ())
        .await?;

    // Receiver reissues the notes
    let reissue_op = receiver_mint.reissue_external_notes(notes, ()).await?;
    let mut sub = receiver_mint
        .subscribe_reissue_external_notes(reissue_op)
        .await?
        .into_stream();

    assert_eq!(sub.ok().await?, ReissueExternalNotesState::Created);
    assert_eq!(sub.ok().await?, ReissueExternalNotesState::Issuing);
    assert_eq!(sub.ok().await?, ReissueExternalNotesState::Done);

    // Verify receiver got the funds
    let receiver_balance = receiver.get_balance_for_btc().await?;
    assert!(
        receiver_balance >= sats(750),
        "Receiver should have at least 750 sats, got {receiver_balance}"
    );

    Ok(())
}

/// Test 4: Escrow pattern — client A locks funds as OOBNotes, client B claims them.
/// This mirrors CREAM's PlaceOrder → FulfillOrder flow.
#[tokio::test(flavor = "multi_thread")]
async fn test_ecash_escrow_pattern() -> Result<()> {
    let fed = fixtures().new_fed_not_degraded().await;
    let (customer, supplier) = fed.two_clients().await;

    issue_ecash(&customer, sats(5000)).await?;

    let customer_mint = customer.get_first_module::<MintClientModule>()?;
    let supplier_mint = supplier.get_first_module::<MintClientModule>()?;

    // Customer locks 2000 sats as escrow (PlaceOrder)
    let (_escrow_op, escrow_notes) = customer_mint
        .spend_notes_with_selector(
            &SelectNotesWithAtleastAmount,
            sats(2000),
            TIMEOUT,
            false,
            (),
        )
        .await?;

    // The OOBNotes are the "escrow token" — in CREAM this would be stored
    // in the Order struct. Serialize to string for storage.
    let token_string = escrow_notes.to_string();
    assert!(!token_string.is_empty(), "Token should serialize to string");

    // Supplier claims the escrow (FulfillOrder)
    let parsed_notes: fedimint_mint_client::OOBNotes = token_string.parse().unwrap();
    let reissue_op = supplier_mint
        .reissue_external_notes(parsed_notes, ())
        .await?;
    let mut sub = supplier_mint
        .subscribe_reissue_external_notes(reissue_op)
        .await?
        .into_stream();

    assert_eq!(sub.ok().await?, ReissueExternalNotesState::Created);
    assert_eq!(sub.ok().await?, ReissueExternalNotesState::Issuing);
    assert_eq!(sub.ok().await?, ReissueExternalNotesState::Done);

    // Supplier got the deposit
    let supplier_balance = supplier.get_balance_for_btc().await?;
    assert!(
        supplier_balance >= sats(2000),
        "Supplier should have at least 2000 sats, got {supplier_balance}"
    );

    Ok(())
}

/// Test 5: Spend notes then cancel before recipient reissues — verify refund.
/// This mirrors CREAM's CancelOrder flow.
#[tokio::test(flavor = "multi_thread")]
async fn test_ecash_cancel_spend() -> Result<()> {
    let fed = fixtures().new_fed_not_degraded().await;
    let client = fed.new_client().await;

    issue_ecash(&client, sats(1000)).await?;
    let initial_balance = client.get_balance_for_btc().await?;

    let mint = client.get_first_module::<MintClientModule>()?;

    // Spend 500 sats (creates OOBNotes but nobody reissues them)
    let (spend_op, _notes) = mint
        .spend_notes_with_selector(&SelectNotesWithAtleastAmount, sats(500), TIMEOUT, false, ())
        .await?;

    let sub = &mut mint.subscribe_spend_notes(spend_op).await?.into_stream();
    assert_eq!(sub.ok().await?, SpendOOBState::Created);

    // Cancel the spend before anyone reissues
    mint.try_cancel_spend_notes(spend_op).await;
    assert_eq!(sub.ok().await?, SpendOOBState::UserCanceledProcessing);
    assert_eq!(sub.ok().await?, SpendOOBState::UserCanceledSuccess);

    // Balance should be restored (minus any fees)
    let final_balance = client.get_balance_for_btc().await?;
    assert!(
        final_balance >= initial_balance - sats(10), // allow small fee tolerance
        "Balance should be restored after cancel: initial={initial_balance}, final={final_balance}"
    );

    Ok(())
}
