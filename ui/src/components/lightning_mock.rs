use std::collections::HashMap;

use cream_common::lightning_gateway::{
    GatewayError, LightningGateway, LnInvoice, PaymentStatus,
};

/// Mock Lightning gateway for development.
///
/// All invoices are instantly "paid" — no real Lightning network involved.
/// Swap this for an LND-backed implementation in production.
#[allow(dead_code)] // used in WASM builds only (node_api PegIn/PegOut handlers)
pub struct MockLightningGateway {
    invoices: HashMap<String, LnInvoice>,
    counter: u64,
}

#[allow(dead_code)] // used in WASM builds only
impl MockLightningGateway {
    pub fn new() -> Self {
        Self {
            invoices: HashMap::new(),
            counter: 0,
        }
    }
}

impl LightningGateway for MockLightningGateway {
    fn create_invoice(&mut self, amount_sats: u64, memo: &str) -> Result<LnInvoice, GatewayError> {
        self.counter += 1;
        let payment_hash = format!("mock_hash_{}", self.counter);
        let invoice = LnInvoice {
            bolt11: format!("lnbc{}mock{}", amount_sats, self.counter),
            amount_sats,
            memo: memo.to_string(),
            payment_hash: payment_hash.clone(),
        };
        self.invoices.insert(payment_hash, invoice.clone());
        Ok(invoice)
    }

    fn pay_invoice(&mut self, _bolt11: &str) -> Result<PaymentStatus, GatewayError> {
        Ok(PaymentStatus::Success {
            preimage: format!("mock_preimage_{}", self.counter),
        })
    }

    fn check_invoice(&self, payment_hash: &str) -> Result<PaymentStatus, GatewayError> {
        if self.invoices.contains_key(payment_hash) {
            Ok(PaymentStatus::Success {
                preimage: format!("mock_preimage_for_{}", payment_hash),
            })
        } else {
            Err(GatewayError::PaymentFailed(format!(
                "unknown payment hash: {}",
                payment_hash
            )))
        }
    }

    fn gateway_name(&self) -> &str {
        "mock"
    }
}
