//! LND Lightning gateway for CREAM guardian nodes.
//!
//! Wraps `tonic_openssl_lnd` to provide hold-invoice peg-in and payment peg-out.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};
use tonic_openssl_lnd::invoicesrpc;
use tonic_openssl_lnd::lnrpc;
use tonic_openssl_lnd::routerrpc;
use tonic_openssl_lnd::LndClient;
use tracing::{error, info};

/// Configuration for the LND connection.
#[derive(Clone, Debug)]
pub struct LndConfig {
    pub host: String,
    pub port: u32,
    pub cert_path: String,
    pub macaroon_path: String,
    pub pegin_limit_sats: Option<u64>,
}

/// Tracks a pending peg-in operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingPegIn {
    pub payment_hash: String,
    pub bolt11: String,
    pub amount_sats: u64,
    pub preimage: String,
    pub created_at: u64,
    pub status: PegInStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PegInStatus {
    Waiting,
    Accepted,
    Settled,
    Cancelled,
}

/// Result of a peg-out payment attempt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PegOutResult {
    pub success: bool,
    pub preimage: Option<String>,
    pub error: Option<String>,
}

/// Transaction log entry for peg-in/peg-out history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PegTransaction {
    pub kind: String,
    pub payment_hash: String,
    pub amount_sats: u64,
    pub status: String,
    pub timestamp: u64,
}

/// LND gateway wrapping the unified `LndClient`.
pub struct LndGateway {
    client: LndClient,
    config: LndConfig,
}

/// Shared Lightning state stored in AppState.
pub struct LightningState {
    pub gateway: Mutex<LndGateway>,
    pub pending_pegins: RwLock<BTreeMap<String, PendingPegIn>>,
    pub used_payment_hashes: RwLock<HashSet<String>>,
    pub peg_history: RwLock<Vec<PegTransaction>>,
    pub persistence_path: PathBuf,
}

impl LndGateway {
    /// Connect to LND via gRPC with TLS cert + macaroon.
    pub async fn connect(config: LndConfig) -> Result<Self, String> {
        let client = tonic_openssl_lnd::connect(
            config.host.clone(),
            config.port,
            config.cert_path.clone(),
            config.macaroon_path.clone(),
        )
        .await
        .map_err(|e| format!("Failed to connect to LND: {}", e))?;

        info!("Connected to LND at {}:{}", config.host, config.port);

        Ok(Self { client, config })
    }

    /// Create a hold invoice. Returns (payment_hash_hex, preimage_hex, bolt11).
    pub async fn create_hold_invoice(
        &mut self,
        amount_sats: u64,
        memo: &str,
    ) -> Result<(String, String, String), String> {
        let preimage: [u8; 32] = rand::random();
        let hash = Sha256::digest(preimage);

        let resp = self
            .client
            .invoices()
            .add_hold_invoice(invoicesrpc::AddHoldInvoiceRequest {
                memo: memo.to_string(),
                hash: hash.to_vec(),
                value: amount_sats as i64,
                expiry: 3600,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("AddHoldInvoice failed: {}", e))?;

        let inner = resp.into_inner();
        Ok((hex_encode(&hash), hex_encode(&preimage), inner.payment_request))
    }

    /// Settle a hold invoice after CURD has been allocated.
    pub async fn settle_invoice(&mut self, preimage_hex: &str) -> Result<(), String> {
        let preimage = hex_decode(preimage_hex)?;
        self.client
            .invoices()
            .settle_invoice(invoicesrpc::SettleInvoiceMsg { preimage })
            .await
            .map_err(|e| format!("SettleInvoice failed: {}", e))?;
        Ok(())
    }

    /// Cancel a hold invoice on failure.
    pub async fn cancel_invoice(&mut self, payment_hash_hex: &str) -> Result<(), String> {
        let payment_hash = hex_decode(payment_hash_hex)?;
        self.client
            .invoices()
            .cancel_invoice(invoicesrpc::CancelInvoiceMsg { payment_hash })
            .await
            .map_err(|e| format!("CancelInvoice failed: {}", e))?;
        Ok(())
    }

    /// Check invoice status by looking it up.
    pub async fn check_invoice_status(
        &mut self,
        payment_hash_hex: &str,
    ) -> Result<PegInStatus, String> {
        let payment_hash = hex_decode(payment_hash_hex)?;
        let resp = self
            .client
            .invoices()
            .lookup_invoice_v2(invoicesrpc::LookupInvoiceMsg {
                invoice_ref: Some(
                    invoicesrpc::lookup_invoice_msg::InvoiceRef::PaymentHash(payment_hash),
                ),
                lookup_modifier: 0,
            })
            .await
            .map_err(|e| format!("LookupInvoice failed: {}", e))?;

        let invoice = resp.into_inner();
        // lnrpc::Invoice::state: 0=OPEN, 1=SETTLED, 2=CANCELED, 3=ACCEPTED
        match invoice.state {
            0 => Ok(PegInStatus::Waiting),
            1 => Ok(PegInStatus::Settled),
            2 => Ok(PegInStatus::Cancelled),
            3 => Ok(PegInStatus::Accepted),
            _ => Ok(PegInStatus::Waiting),
        }
    }

    /// Pay a BOLT11 invoice for peg-out. Uses SendPaymentV2 (streaming).
    pub async fn pay_invoice(&mut self, bolt11: &str) -> Result<PegOutResult, String> {
        use tokio_stream::StreamExt;

        let mut stream = self
            .client
            .router()
            .send_payment_v2(routerrpc::SendPaymentRequest {
                payment_request: bolt11.to_string(),
                timeout_seconds: 60,
                fee_limit_sat: 100,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("SendPaymentV2 failed: {}", e))?
            .into_inner();

        while let Some(update) = stream.next().await {
            match update {
                Ok(payment) => {
                    // lnrpc::Payment::status: 0=UNKNOWN, 1=IN_FLIGHT, 2=SUCCEEDED, 3=FAILED
                    match payment.status {
                        2 => {
                            return Ok(PegOutResult {
                                success: true,
                                preimage: Some(payment.payment_preimage),
                                error: None,
                            });
                        }
                        3 => {
                            return Ok(PegOutResult {
                                success: false,
                                preimage: None,
                                error: Some(payment.failure_reason.to_string()),
                            });
                        }
                        _ => continue,
                    }
                }
                Err(e) => {
                    return Ok(PegOutResult {
                        success: false,
                        preimage: None,
                        error: Some(format!("Stream error: {}", e)),
                    });
                }
            }
        }

        Ok(PegOutResult {
            success: false,
            preimage: None,
            error: Some("Payment stream ended without terminal status".to_string()),
        })
    }

    /// Get LND node info (pubkey, alias, synced).
    pub async fn get_info(&mut self) -> Result<LndInfo, String> {
        let resp = self
            .client
            .lightning()
            .get_info(lnrpc::GetInfoRequest {})
            .await
            .map_err(|e| format!("GetInfo failed: {}", e))?;

        let info = resp.into_inner();
        Ok(LndInfo {
            pubkey: info.identity_pubkey,
            alias: info.alias,
            synced_to_chain: info.synced_to_chain,
            synced_to_graph: info.synced_to_graph,
            block_height: info.block_height,
            num_active_channels: info.num_active_channels,
            num_peers: info.num_peers,
        })
    }

    /// Get wallet balance (on-chain).
    pub async fn wallet_balance(&mut self) -> Result<WalletBalance, String> {
        let resp = self
            .client
            .lightning()
            .wallet_balance(lnrpc::WalletBalanceRequest {})
            .await
            .map_err(|e| format!("WalletBalance failed: {}", e))?;

        let bal = resp.into_inner();
        Ok(WalletBalance {
            total_balance: bal.total_balance,
            confirmed_balance: bal.confirmed_balance,
            unconfirmed_balance: bal.unconfirmed_balance,
        })
    }

    /// Get channel balance summary.
    pub async fn channel_balance(&mut self) -> Result<ChannelBalance, String> {
        let resp = self
            .client
            .lightning()
            .channel_balance(lnrpc::ChannelBalanceRequest {})
            .await
            .map_err(|e| format!("ChannelBalance failed: {}", e))?;

        let bal = resp.into_inner();
        Ok(ChannelBalance {
            local_balance_sat: bal.local_balance.as_ref().map(|a| a.sat).unwrap_or(0),
            remote_balance_sat: bal.remote_balance.as_ref().map(|a| a.sat).unwrap_or(0),
        })
    }

    /// List active channels.
    pub async fn list_channels(&mut self) -> Result<Vec<ChannelInfo>, String> {
        let resp = self
            .client
            .lightning()
            .list_channels(lnrpc::ListChannelsRequest {
                ..Default::default()
            })
            .await
            .map_err(|e| format!("ListChannels failed: {}", e))?;

        Ok(resp
            .into_inner()
            .channels
            .into_iter()
            .map(|ch| ChannelInfo {
                channel_point: ch.channel_point,
                remote_pubkey: ch.remote_pubkey,
                capacity: ch.capacity,
                local_balance: ch.local_balance,
                remote_balance: ch.remote_balance,
                active: ch.active,
            })
            .collect())
    }

    /// Open a channel with a peer.
    pub async fn open_channel(
        &mut self,
        node_pubkey_hex: &str,
        local_funding_amount: i64,
    ) -> Result<String, String> {
        let node_pubkey = hex_decode(node_pubkey_hex)?;
        let resp = self
            .client
            .lightning()
            .open_channel_sync(lnrpc::OpenChannelRequest {
                node_pubkey,
                local_funding_amount,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("OpenChannel failed: {}", e))?;

        let point = resp.into_inner();
        let txid = match point.funding_txid {
            Some(lnrpc::channel_point::FundingTxid::FundingTxidStr(s)) => s,
            Some(lnrpc::channel_point::FundingTxid::FundingTxidBytes(b)) => {
                let mut reversed = b;
                reversed.reverse();
                hex_encode(&reversed)
            }
            None => "unknown".to_string(),
        };
        Ok(format!("{}:{}", txid, point.output_index))
    }

    /// Close a channel.
    pub async fn close_channel(
        &mut self,
        channel_point_str: &str,
        force: bool,
    ) -> Result<(), String> {
        use tokio_stream::StreamExt;

        let parts: Vec<&str> = channel_point_str.split(':').collect();
        if parts.len() != 2 {
            return Err("Invalid channel_point format, expected txid:output_index".to_string());
        }
        let output_index: u32 = parts[1]
            .parse()
            .map_err(|e| format!("Invalid output_index: {}", e))?;

        let channel_point = lnrpc::ChannelPoint {
            funding_txid: Some(lnrpc::channel_point::FundingTxid::FundingTxidStr(
                parts[0].to_string(),
            )),
            output_index,
        };

        let mut stream = self
            .client
            .lightning()
            .close_channel(lnrpc::CloseChannelRequest {
                channel_point: Some(channel_point),
                force,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("CloseChannel failed: {}", e))?
            .into_inner();

        if let Some(update) = stream.next().await {
            match update {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("CloseChannel stream error: {}", e)),
            }
        } else {
            Ok(())
        }
    }

    /// Check peg-in limit.
    pub fn check_pegin_limit(&self, amount_sats: u64) -> Result<(), String> {
        if let Some(limit) = self.config.pegin_limit_sats {
            if amount_sats > limit {
                return Err(format!(
                    "Peg-in amount {} sats exceeds limit of {} sats",
                    amount_sats, limit
                ));
            }
        }
        Ok(())
    }
}

impl LightningState {
    pub fn new(gateway: LndGateway, share_index: u16) -> Self {
        let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        let persistence_path = cache
            .join("freenet")
            .join(format!("guardian-{}", share_index))
            .join("used_payment_hashes.json");

        let used_hashes = load_used_hashes(&persistence_path);
        info!(
            "Loaded {} used payment hashes from {}",
            used_hashes.len(),
            persistence_path.display()
        );

        Self {
            gateway: Mutex::new(gateway),
            pending_pegins: RwLock::new(BTreeMap::new()),
            used_payment_hashes: RwLock::new(used_hashes),
            peg_history: RwLock::new(Vec::new()),
            persistence_path,
        }
    }

    /// Record a payment hash as used and persist to disk.
    pub async fn mark_hash_used(&self, payment_hash: &str) {
        let mut hashes = self.used_payment_hashes.write().await;
        hashes.insert(payment_hash.to_string());
        if let Err(e) = save_used_hashes(&self.persistence_path, &hashes) {
            error!("Failed to persist used payment hashes: {}", e);
        }
    }

    /// Check if a payment hash has already been used.
    pub async fn is_hash_used(&self, payment_hash: &str) -> bool {
        self.used_payment_hashes
            .read()
            .await
            .contains(payment_hash)
    }

    /// Record a peg transaction in history.
    pub async fn record_peg_event(
        &self,
        kind: &str,
        payment_hash: &str,
        amount_sats: u64,
        status: &str,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut history = self.peg_history.write().await;
        history.push(PegTransaction {
            kind: kind.to_string(),
            payment_hash: payment_hash.to_string(),
            amount_sats,
            status: status.to_string(),
            timestamp: now,
        });
    }
}

// ─── Response types ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LndInfo {
    pub pubkey: String,
    pub alias: String,
    pub synced_to_chain: bool,
    pub synced_to_graph: bool,
    pub block_height: u32,
    pub num_active_channels: u32,
    pub num_peers: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletBalance {
    pub total_balance: i64,
    pub confirmed_balance: i64,
    pub unconfirmed_balance: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelBalance {
    pub local_balance_sat: u64,
    pub remote_balance_sat: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub channel_point: String,
    pub remote_pubkey: String,
    pub capacity: i64,
    pub local_balance: i64,
    pub remote_balance: i64,
    pub active: bool,
}

// ─── Persistence ─────────────────────────────────────────────────────────────

fn load_used_hashes(path: &PathBuf) -> HashSet<String> {
    match std::fs::read_to_string(path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => HashSet::new(),
    }
}

fn save_used_hashes(path: &PathBuf, hashes: &HashSet<String>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let data = serde_json::to_string(hashes).map_err(|e| format!("serialize: {}", e))?;
    std::fs::write(path, data).map_err(|e| format!("write: {}", e))?;
    Ok(())
}

// ─── Hex helpers ─────────────────────────────────────────────────────────────

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("Odd-length hex string".to_string());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| format!("Invalid hex at position {}: {}", i, e))
        })
        .collect()
}
