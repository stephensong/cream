#![cfg(feature = "guardian-tests")]

//! Guardian stress tests — exercises the guardian HTTP protocol for signing,
//! refresh, redeal, concurrency, and crash recovery.

use frost_ed25519 as frost;
use frost_ed25519::keys::PublicKeyPackage;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use tempfile::TempDir;

// ─── API types (mirrored from guardian) ──────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct Round1Request {
    session_id: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Round1Response {
    identifier: frost::Identifier,
    commitments: frost::round1::SigningCommitments,
}

#[derive(Serialize)]
struct Round2Request {
    session_id: String,
    message_hex: String,
    signing_commitments: Vec<Round1Response>,
}

#[derive(Deserialize)]
struct Round2Response {
    identifier: frost::Identifier,
    signature_share: frost::round2::SignatureShare,
}

#[derive(Deserialize)]
struct ConfigResponse {
    min_signers: u16,
    #[allow(dead_code)]
    max_signers: u16,
}

#[derive(Deserialize)]
struct HealthResponse {
    #[allow(dead_code)]
    status: String,
    #[allow(dead_code)]
    identifier: frost::Identifier,
    ready: bool,
    #[allow(dead_code)]
    refreshing: bool,
    #[allow(dead_code)]
    node_connected: bool,
}

// ─── GuardianCluster ─────────────────────────────────────────────────────────

/// Manages a cluster of guardian processes with isolated HOME directories.
struct GuardianCluster {
    children: Vec<Option<Child>>,
    ports: Vec<u16>,
    home_dir: TempDir,
    guardian_bin: PathBuf,
    max_signers: u16,
    min_signers: u16,
}

impl GuardianCluster {
    /// Find the guardian binary (must be pre-built via `build-guardian`).
    fn guardian_bin() -> PathBuf {
        let mut path = std::env::current_dir().unwrap();
        // Walk up until we find Cargo.toml at workspace root
        loop {
            if path.join("Makefile.toml").exists() {
                break;
            }
            if !path.pop() {
                panic!("Could not find workspace root (Makefile.toml)");
            }
        }
        let bin = path.join("target/debug/cream-guardian");
        assert!(
            bin.exists(),
            "Guardian binary not found at {:?}. Run `cargo make build-guardian` first.",
            bin
        );
        bin
    }

    /// Start N guardians with DKG ceremony.
    async fn start_with_dkg(n: u16, min_signers: u16, base_port: u16) -> Self {
        let home_dir = TempDir::new().expect("failed to create tempdir");
        let guardian_bin = Self::guardian_bin();
        let mut children = Vec::new();
        let ports: Vec<u16> = (0..n).map(|i| base_port + i).collect();

        // Build peer lists for each guardian
        for i in 0..n {
            let my_port = ports[i as usize];
            let peers: Vec<String> = ports
                .iter()
                .filter(|&&p| p != my_port)
                .map(|p| format!("http://localhost:{}", p))
                .collect();
            let peers_str = peers.join(",");

            // Each guardian gets its own cache dir under the tempdir
            let cache_dir = home_dir.path().join("cache").join("freenet");
            std::fs::create_dir_all(&cache_dir).unwrap();

            let child = Command::new(&guardian_bin)
                .arg("--share-index")
                .arg(format!("{}", i + 1))
                .arg("--port")
                .arg(format!("{}", my_port))
                .arg("--max-signers")
                .arg(format!("{}", n))
                .arg("--min-signers")
                .arg(format!("{}", min_signers))
                .arg("--peers")
                .arg(&peers_str)
                .env("HOME", home_dir.path())
                .env("XDG_CACHE_HOME", home_dir.path().join("cache"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap_or_else(|e| panic!("Failed to start guardian {}: {}", i + 1, e));

            children.push(Some(child));
        }

        let cluster = Self {
            children,
            ports: ports.clone(),
            home_dir,
            guardian_bin,
            max_signers: n,
            min_signers,
        };

        // Wait for all guardians to become ready
        cluster.wait_all_ready(Duration::from_secs(60)).await;
        cluster
    }

    /// Wait for all living guardians to report ready=true.
    async fn wait_all_ready(&self, timeout: Duration) {
        let client = Client::new();
        let deadline = Instant::now() + timeout;

        for (i, port) in self.ports.iter().enumerate() {
            if self.children[i].is_none() {
                continue;
            }
            loop {
                if Instant::now() > deadline {
                    panic!(
                        "Guardian {} (port {}) did not become ready within {:?}",
                        i + 1,
                        port,
                        timeout
                    );
                }
                if let Ok(resp) = client
                    .get(format!("http://localhost:{}/health", port))
                    .send()
                    .await
                {
                    if let Ok(health) = resp.json::<HealthResponse>().await {
                        if health.ready {
                            eprintln!("Guardian {} ready on port {}", i + 1, port);
                            break;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }

    /// Wait for a specific guardian to report ready=true.
    async fn wait_one_ready(&self, idx: usize, timeout: Duration) {
        let client = Client::new();
        let deadline = Instant::now() + timeout;
        let port = self.ports[idx];

        loop {
            if Instant::now() > deadline {
                panic!(
                    "Guardian {} (port {}) did not become ready within {:?}",
                    idx + 1,
                    port,
                    timeout
                );
            }
            if let Ok(resp) = client
                .get(format!("http://localhost:{}/health", port))
                .send()
                .await
            {
                if let Ok(health) = resp.json::<HealthResponse>().await {
                    if health.ready {
                        eprintln!("Guardian {} ready on port {}", idx + 1, port);
                        return;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Kill one guardian, return its key directory path for restart.
    fn kill_one(&mut self, idx: usize) -> PathBuf {
        if let Some(ref mut child) = self.children[idx] {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.children[idx] = None;
        // Keys are stored at $XDG_CACHE_HOME/freenet/guardian-{idx+1}/
        self.home_dir
            .path()
            .join("cache")
            .join("freenet")
            .join(format!("guardian-{}", idx + 1))
    }

    /// Restart a killed guardian from persisted keys on disk.
    async fn restart_from_disk(&mut self, idx: usize) {
        let port = self.ports[idx];
        let child = Command::new(&self.guardian_bin)
            .arg("--share-index")
            .arg(format!("{}", idx + 1))
            .arg("--port")
            .arg(format!("{}", port))
            .arg("--max-signers")
            .arg(format!("{}", self.max_signers))
            .arg("--min-signers")
            .arg(format!("{}", self.min_signers))
            .env("HOME", self.home_dir.path())
            .env("XDG_CACHE_HOME", self.home_dir.path().join("cache"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("Failed to restart guardian {}: {}", idx + 1, e));

        self.children[idx] = Some(child);
        self.wait_one_ready(idx, Duration::from_secs(30)).await;
    }

    /// Kill all guardians, restart with --refresh --peers.
    async fn restart_with_refresh(&mut self) {
        // Kill all
        for i in 0..self.children.len() {
            if let Some(ref mut child) = self.children[i] {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.children[i] = None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Restart with --refresh
        for i in 0..self.ports.len() {
            let my_port = self.ports[i];
            let peers: Vec<String> = self
                .ports
                .iter()
                .filter(|&&p| p != my_port)
                .map(|p| format!("http://localhost:{}", p))
                .collect();
            let peers_str = peers.join(",");

            let child = Command::new(&self.guardian_bin)
                .arg("--share-index")
                .arg(format!("{}", i + 1))
                .arg("--port")
                .arg(format!("{}", my_port))
                .arg("--max-signers")
                .arg(format!("{}", self.max_signers))
                .arg("--min-signers")
                .arg(format!("{}", self.min_signers))
                .arg("--refresh")
                .arg("--peers")
                .arg(&peers_str)
                .env("HOME", self.home_dir.path())
                .env("XDG_CACHE_HOME", self.home_dir.path().join("cache"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap_or_else(|e| panic!("Failed to restart guardian {} for refresh: {}", i + 1, e));

            self.children[i] = Some(child);
        }

        self.wait_all_ready(Duration::from_secs(60)).await;
    }

    /// Start additional guardians (for redeal topology expansion).
    fn start_additional(&mut self, count: u16, new_base_port: u16) {
        let start_idx = self.children.len();
        for i in 0..count {
            let port = new_base_port + i;
            let share_index = start_idx as u16 + i + 1;

            let child = Command::new(&self.guardian_bin)
                .arg("--share-index")
                .arg(format!("{}", share_index))
                .arg("--port")
                .arg(format!("{}", port))
                .arg("--max-signers")
                .arg(format!("{}", self.max_signers))
                .arg("--min-signers")
                .arg(format!("{}", self.min_signers))
                .env("HOME", self.home_dir.path())
                .env("XDG_CACHE_HOME", self.home_dir.path().join("cache"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap_or_else(|e| {
                    panic!("Failed to start additional guardian {}: {}", share_index, e)
                });

            self.children.push(Some(child));
            self.ports.push(port);
        }
    }

    /// Get the URLs for all living guardians.
    fn live_urls(&self) -> Vec<String> {
        self.ports
            .iter()
            .enumerate()
            .filter(|(i, _)| self.children[*i].is_some())
            .map(|(_, p)| format!("http://localhost:{}", p))
            .collect()
    }

    /// Get all guardian URLs.
    fn all_urls(&self) -> Vec<String> {
        self.ports
            .iter()
            .map(|p| format!("http://localhost:{}", p))
            .collect()
    }
}

impl Drop for GuardianCluster {
    fn drop(&mut self) {
        for child in &mut self.children {
            if let Some(ref mut c) = child {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

// ─── GuardianClient ──────────────────────────────────────────────────────────

/// Native FROST signing coordinator (mirrors the WASM `signing_service.rs` logic).
struct GuardianClient {
    client: Client,
    guardian_urls: Vec<String>,
    min_signers: u16,
    public_key_package: PublicKeyPackage,
}

impl GuardianClient {
    /// Create a new client, fetching config and public key from the first guardian.
    async fn new(guardian_urls: Vec<String>) -> Self {
        Self::try_new(guardian_urls)
            .await
            .expect("Failed to create GuardianClient")
    }

    /// Fallible constructor — returns Err if guardians are unreachable.
    async fn try_new(guardian_urls: Vec<String>) -> Result<Self, String> {
        let client = Client::new();

        // Try each guardian URL until we get config + public key
        let mut last_err = String::new();
        for url in &guardian_urls {
            let config = match client
                .get(format!("{}/config", url))
                .send()
                .await
            {
                Ok(resp) => match resp.json::<ConfigResponse>().await {
                    Ok(c) => c,
                    Err(e) => {
                        last_err = format!("parse /config from {}: {}", url, e);
                        continue;
                    }
                },
                Err(e) => {
                    last_err = format!("reach {}/config: {}", url, e);
                    continue;
                }
            };

            let public_key_package = match client
                .get(format!("{}/public-key", url))
                .send()
                .await
            {
                Ok(resp) => match resp.json::<PublicKeyPackage>().await {
                    Ok(pkg) => pkg,
                    Err(e) => {
                        last_err = format!("parse /public-key from {}: {}", url, e);
                        continue;
                    }
                },
                Err(e) => {
                    last_err = format!("reach {}/public-key: {}", url, e);
                    continue;
                }
            };

            return Ok(Self {
                client,
                guardian_urls,
                min_signers: config.min_signers,
                public_key_package,
            });
        }

        Err(format!("No guardian reachable. Last error: {}", last_err))
    }

    /// Refresh the cached public key from guardians.
    #[allow(dead_code)]
    async fn refresh_public_key(&mut self) {
        for url in &self.guardian_urls {
            if let Ok(resp) = self
                .client
                .get(format!("{}/public-key", url))
                .send()
                .await
            {
                if let Ok(pkg) = resp.json::<PublicKeyPackage>().await {
                    self.public_key_package = pkg;
                    return;
                }
            }
        }
        panic!("Could not fetch public key from any guardian");
    }

    /// Refresh config (min_signers) from guardians.
    #[allow(dead_code)]
    async fn refresh_config(&mut self) {
        for url in &self.guardian_urls {
            if let Ok(resp) = self.client.get(format!("{}/config", url)).send().await {
                if let Ok(config) = resp.json::<ConfigResponse>().await {
                    self.min_signers = config.min_signers;
                    return;
                }
            }
        }
        panic!("Could not fetch config from any guardian");
    }

    /// Perform a complete FROST signing ceremony and return the ed25519 signature.
    async fn sign(&self, message: &[u8]) -> Result<frost::Signature, String> {
        let session_id = generate_session_id();
        let message_hex = hex::encode(message);

        // Round 1: collect commitments from all guardians
        let mut round1_results = Vec::new();
        let mut round1_errors = Vec::new();

        for url in &self.guardian_urls {
            let req = Round1Request {
                session_id: session_id.clone(),
            };
            match self
                .client
                .post(format!("{}/round1", url))
                .json(&req)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Round1Response>().await {
                        Ok(r1) => round1_results.push((url.clone(), r1)),
                        Err(e) => round1_errors.push(format!("{}: parse error: {}", url, e)),
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    round1_errors.push(format!("{}: HTTP {} - {}", url, status, body));
                }
                Err(e) => round1_errors.push(format!("{}: {}", url, e)),
            }
        }

        if (round1_results.len() as u16) < self.min_signers {
            return Err(format!(
                "Only {} of {} guardians responded to round1 (need {}). Errors: {}",
                round1_results.len(),
                self.guardian_urls.len(),
                self.min_signers,
                round1_errors.join("; ")
            ));
        }

        // Take exactly min_signers participants
        let participants: Vec<(String, Round1Response)> = round1_results
            .into_iter()
            .take(self.min_signers as usize)
            .collect();
        let all_commitments: Vec<Round1Response> =
            participants.iter().map(|(_, r)| r.clone()).collect();

        // Round 2: send commitments + message to each participant
        let mut signature_shares: BTreeMap<frost::Identifier, frost::round2::SignatureShare> =
            BTreeMap::new();
        let mut round2_errors = Vec::new();

        for (url, _) in &participants {
            let req = Round2Request {
                session_id: session_id.clone(),
                message_hex: message_hex.clone(),
                signing_commitments: all_commitments.clone(),
            };
            match self
                .client
                .post(format!("{}/round2", url))
                .json(&req)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Round2Response>().await {
                        Ok(r2) => {
                            signature_shares.insert(r2.identifier, r2.signature_share);
                        }
                        Err(e) => round2_errors.push(format!("{}: parse error: {}", url, e)),
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    round2_errors.push(format!("{}: HTTP {} - {}", url, status, body));
                }
                Err(e) => round2_errors.push(format!("{}: {}", url, e)),
            }
        }

        if (signature_shares.len() as u16) < self.min_signers {
            return Err(format!(
                "Only {} of {} guardians completed round2 (need {}). Errors: {}",
                signature_shares.len(),
                self.min_signers,
                self.min_signers,
                round2_errors.join("; ")
            ));
        }

        // Build commitments map for aggregation
        let commitments_map: BTreeMap<frost::Identifier, frost::round1::SigningCommitments> =
            all_commitments
                .into_iter()
                .map(|c| (c.identifier, c.commitments))
                .collect();

        // Aggregate
        let signing_package = frost::SigningPackage::new(commitments_map, message);
        frost::aggregate(
            &signing_package,
            &signature_shares,
            &self.public_key_package,
        )
        .map_err(|e| format!("FROST aggregation failed: {}", e))
    }

    /// Verify a FROST signature against the group verifying key.
    fn verify(&self, message: &[u8], signature: &frost::Signature) -> bool {
        let verifying_key = self.public_key_package.verifying_key();
        verifying_key.verify(message, signature).is_ok()
    }
}

fn generate_session_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    hex::encode(bytes)
}

// ─── Test E: Crash Recovery (validates cluster infrastructure) ───────────────

#[tokio::test]
async fn crash_recovery() {
    eprintln!("\n── Test E: crash_recovery ──");

    let mut cluster = GuardianCluster::start_with_dkg(3, 2, 4050).await;
    let client = GuardianClient::new(cluster.live_urls()).await;

    // Sign a message with all 3
    let msg = b"crash recovery test - before crash";
    let sig = client.sign(msg).await.expect("signing before crash failed");
    assert!(client.verify(msg, &sig), "signature verification failed before crash");
    eprintln!("  Signed successfully with all 3 guardians");

    // Kill guardian 1
    let _key_dir = cluster.kill_one(0);
    eprintln!("  Killed guardian 1");

    // Sign with remaining 2 (at threshold)
    let urls_without_1: Vec<String> = cluster.live_urls();
    assert_eq!(urls_without_1.len(), 2);
    let client_2of3 = GuardianClient::new(urls_without_1).await;

    let msg2 = b"crash recovery test - with 2 of 3";
    let sig2 = client_2of3
        .sign(msg2)
        .await
        .expect("signing with 2-of-3 failed");
    assert!(
        client_2of3.verify(msg2, &sig2),
        "signature verification failed with 2-of-3"
    );
    eprintln!("  Signed successfully with 2-of-3 (at threshold)");

    // Restart guardian 1 from disk
    cluster.restart_from_disk(0).await;
    eprintln!("  Restarted guardian 1 from persisted keys");

    // Sign with all 3 again
    let client_restored = GuardianClient::new(cluster.all_urls()).await;
    let msg3 = b"crash recovery test - after restart";
    let sig3 = client_restored
        .sign(msg3)
        .await
        .expect("signing after restart failed");
    assert!(
        client_restored.verify(msg3, &sig3),
        "signature verification failed after restart"
    );
    eprintln!("  Signed successfully with all 3 after restart");
    eprintln!("── Test E: crash_recovery ── PASSED");
}

// ─── Test A: Concurrent Signing ──────────────────────────────────────────────

#[tokio::test]
async fn concurrent_signing() {
    eprintln!("\n── Test A: concurrent_signing ──");

    let cluster = GuardianCluster::start_with_dkg(3, 2, 4010).await;

    for n in [10, 50, 100] {
        let start = Instant::now();
        let mut handles = Vec::new();
        let mut latencies = Vec::new();

        // Fire N concurrent signing sessions
        for i in 0..n {
            let urls = cluster.live_urls();
            handles.push(tokio::spawn(async move {
                let c = GuardianClient::new(urls).await;
                let msg = format!("concurrent test message {}", i);
                let t0 = Instant::now();
                let sig = c.sign(msg.as_bytes()).await?;
                let elapsed = t0.elapsed();
                if !c.verify(msg.as_bytes(), &sig) {
                    return Err("signature verification failed".to_string());
                }
                Ok::<Duration, String>(elapsed)
            }));
        }

        let mut successes = 0u32;
        let mut failures = 0u32;
        for handle in handles {
            match handle.await.unwrap() {
                Ok(lat) => {
                    latencies.push(lat);
                    successes += 1;
                }
                Err(e) => {
                    eprintln!("  [WARN] signing failed: {}", e);
                    failures += 1;
                }
            }
        }

        let total_elapsed = start.elapsed();
        latencies.sort();

        let throughput = successes as f64 / total_elapsed.as_secs_f64();
        let p50 = latencies.get(latencies.len() / 2).copied().unwrap_or_default();
        let p95 = latencies
            .get((latencies.len() as f64 * 0.95) as usize)
            .copied()
            .unwrap_or_default();
        let p99 = latencies
            .get((latencies.len() as f64 * 0.99) as usize)
            .copied()
            .unwrap_or_default();

        eprintln!(
            "  [PERF] N={:<4} | throughput={:.1}/s | p50={:?} | p95={:?} | p99={:?} | ok={} fail={}",
            n, throughput, p50, p95, p99, successes, failures
        );

        // All should succeed
        assert_eq!(
            failures, 0,
            "Expected 0 failures for N={}, got {}",
            n, failures
        );
    }

    eprintln!("── Test A: concurrent_signing ── PASSED");
}

// ─── Test B: Signing Continuity Across Refresh ───────────────────────────────

#[tokio::test]
async fn signing_continuity_across_refresh() {
    eprintln!("\n── Test B: signing_continuity_across_refresh ──");

    let mut cluster = GuardianCluster::start_with_dkg(3, 2, 4020).await;
    let client_before = GuardianClient::new(cluster.live_urls()).await;

    // Sign message A before refresh
    let msg_a = b"message before refresh";
    let sig_a = client_before
        .sign(msg_a)
        .await
        .expect("signing before refresh failed");
    assert!(
        client_before.verify(msg_a, &sig_a),
        "sig A verification failed"
    );
    let vk_before = client_before.public_key_package.verifying_key().clone();
    eprintln!("  Signed message A before refresh");

    // Perform refresh
    cluster.restart_with_refresh().await;
    eprintln!("  Refresh complete");

    // Sign message B after refresh
    let client_after = GuardianClient::new(cluster.live_urls()).await;
    let msg_b = b"message after refresh";
    let sig_b = client_after
        .sign(msg_b)
        .await
        .expect("signing after refresh failed");
    assert!(
        client_after.verify(msg_b, &sig_b),
        "sig B verification failed"
    );
    eprintln!("  Signed message B after refresh");

    // Verify same group key
    let vk_after = client_after.public_key_package.verifying_key().clone();
    assert_eq!(
        vk_before, vk_after,
        "Group verifying key changed after refresh!"
    );

    // Verify sig A still verifies with post-refresh key (it's the same key)
    assert!(
        client_after.verify(msg_a, &sig_a),
        "Pre-refresh sig A should verify with post-refresh key"
    );

    eprintln!("  Group verifying key unchanged");
    eprintln!("── Test B: signing_continuity_across_refresh ── PASSED");
}

// ─── Test C: Signing Continuity Across Redeal ────────────────────────────────

#[tokio::test]
async fn signing_continuity_across_redeal() {
    eprintln!("\n── Test C: signing_continuity_across_redeal ──");

    // Phase 1: DKG with 2-of-3
    let mut cluster = GuardianCluster::start_with_dkg(3, 2, 4030).await;
    let client_before = GuardianClient::new(cluster.live_urls()).await;

    // Sign before redeal
    let msg_before = b"message before redeal";
    let sig_before = client_before
        .sign(msg_before)
        .await
        .expect("signing before redeal failed");
    assert!(
        client_before.verify(msg_before, &sig_before),
        "pre-redeal sig verification failed"
    );
    let vk_before = client_before.public_key_package.verifying_key().clone();
    eprintln!("  Signed message before redeal (2-of-3)");

    // Phase 2: Start 2 additional guardians (indices 4, 5) that wait for redeal
    cluster.start_additional(2, 4033);
    // Wait for them to be listening (not ready, just healthy endpoint responding)
    let http = Client::new();
    for port in [4033u16, 4034] {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if Instant::now() > deadline {
                panic!("Additional guardian on port {} never started", port);
            }
            if http
                .get(format!("http://localhost:{}/health", port))
                .send()
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    eprintln!("  Started 2 additional guardians (ports 4033, 4034)");

    // Phase 3: Kill guardian 1, restart as redeal coordinator
    cluster.kill_one(0);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let old_peers = "http://localhost:4031,http://localhost:4032";
    let new_peers = "http://localhost:4031,http://localhost:4032,http://localhost:4033,http://localhost:4034";

    let child = Command::new(&cluster.guardian_bin)
        .arg("--share-index")
        .arg("1")
        .arg("--port")
        .arg("4030")
        .arg("--redeal")
        .arg("--old-peers")
        .arg(old_peers)
        .arg("--peers")
        .arg(new_peers)
        .arg("--new-max-signers")
        .arg("5")
        .arg("--new-min-signers")
        .arg("3")
        .env("HOME", cluster.home_dir.path())
        .env("XDG_CACHE_HOME", cluster.home_dir.path().join("cache"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start redeal coordinator");

    cluster.children[0] = Some(child);
    cluster.max_signers = 5;
    cluster.min_signers = 3;

    // Wait for all 5 to be ready
    cluster.wait_all_ready(Duration::from_secs(60)).await;
    eprintln!("  Redeal complete (2-of-3 → 3-of-5)");

    // Sign with 3 of 5 (use first 3 URLs as a quick test)
    let sign_urls: Vec<String> = cluster.all_urls().into_iter().take(3).collect();
    let mut client_after = GuardianClient::new(sign_urls).await;
    client_after.min_signers = 3;

    let msg_after = b"message after redeal";
    let sig_after = client_after
        .sign(msg_after)
        .await
        .expect("signing after redeal failed");
    assert!(
        client_after.verify(msg_after, &sig_after),
        "post-redeal sig verification failed"
    );
    eprintln!("  Signed message after redeal (3-of-5)");

    // Verify same group key
    let vk_after = client_after.public_key_package.verifying_key().clone();
    assert_eq!(
        vk_before, vk_after,
        "Group verifying key changed after redeal!"
    );

    // Pre-redeal sig should still verify
    assert!(
        client_after.verify(msg_before, &sig_before),
        "Pre-redeal sig should verify with post-redeal key"
    );

    eprintln!("  Group verifying key unchanged");
    eprintln!("── Test C: signing_continuity_across_redeal ── PASSED");
}

// ─── Test D: Signing During Refresh ──────────────────────────────────────────

#[tokio::test]
async fn signing_during_refresh() {
    eprintln!("\n── Test D: signing_during_refresh ──");

    let mut cluster = GuardianCluster::start_with_dkg(3, 2, 4040).await;
    let urls = cluster.live_urls();
    let vk_before = GuardianClient::new(urls.clone())
        .await
        .public_key_package
        .verifying_key()
        .clone();

    // Background signing loop: 1 sign per 100ms for 15 seconds
    let sign_urls = urls.clone();
    let signing_handle = tokio::spawn(async move {
        let mut successes = 0u32;
        let mut failures = 0u32;
        let start = Instant::now();
        let mut i = 0u32;

        while start.elapsed() < Duration::from_secs(15) {
            let result = match GuardianClient::try_new(sign_urls.clone()).await {
                Ok(client) => {
                    let msg = format!("during-refresh-msg-{}", i);
                    match client.sign(msg.as_bytes()).await {
                        Ok(sig) => {
                            if client.verify(msg.as_bytes(), &sig) {
                                Ok(())
                            } else {
                                Err(format!("sig {} verified=false", i))
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            };

            match result {
                Ok(()) => successes += 1,
                Err(e) => {
                    failures += 1;
                    if failures <= 5 {
                        eprintln!("    [WARN] attempt {} failed: {}", i, e);
                    }
                }
            }
            i += 1;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        (successes, failures, i)
    });

    // Wait 2 seconds, then trigger refresh
    tokio::time::sleep(Duration::from_secs(2)).await;
    eprintln!("  Triggering refresh at 2s mark...");

    // Kill all guardians and restart with --refresh
    // This is the aggressive approach: signing loop will see failures during the restart window
    for i in 0..cluster.children.len() {
        if let Some(ref mut child) = cluster.children[i] {
            let _ = child.kill();
            let _ = child.wait();
        }
        cluster.children[i] = None;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Restart with refresh
    for i in 0..cluster.ports.len() {
        let my_port = cluster.ports[i];
        let peers: Vec<String> = cluster
            .ports
            .iter()
            .filter(|&&p| p != my_port)
            .map(|p| format!("http://localhost:{}", p))
            .collect();
        let peers_str = peers.join(",");

        let child = Command::new(&cluster.guardian_bin)
            .arg("--share-index")
            .arg(format!("{}", i + 1))
            .arg("--port")
            .arg(format!("{}", my_port))
            .arg("--max-signers")
            .arg(format!("{}", cluster.max_signers))
            .arg("--min-signers")
            .arg(format!("{}", cluster.min_signers))
            .arg("--refresh")
            .arg("--peers")
            .arg(&peers_str)
            .env("HOME", cluster.home_dir.path())
            .env("XDG_CACHE_HOME", cluster.home_dir.path().join("cache"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        cluster.children[i] = Some(child);
    }

    // Wait for signing loop to finish
    let (successes, failures, total) = signing_handle.await.unwrap();
    let success_rate = successes as f64 / total as f64 * 100.0;

    eprintln!(
        "  [RESULT] {} succeeded, {} failed out of {} total ({:.1}% success rate)",
        successes, failures, total, success_rate
    );

    // We expect some failures during the restart window, but >50% should succeed
    // (the 2s pre-refresh + post-refresh recovery should dominate the 15s window)
    assert!(
        success_rate > 50.0,
        "Success rate {:.1}% too low (expected >50%)",
        success_rate
    );

    // After refresh, verify signing works and same group key
    cluster.wait_all_ready(Duration::from_secs(30)).await;
    let client_after = GuardianClient::new(cluster.all_urls()).await;
    let vk_after = client_after.public_key_package.verifying_key().clone();

    let msg_final = b"post-refresh verification";
    let sig_final = client_after
        .sign(msg_final)
        .await
        .expect("post-refresh signing failed");
    assert!(
        client_after.verify(msg_final, &sig_final),
        "post-refresh sig verification failed"
    );
    assert_eq!(
        vk_before, vk_after,
        "Group verifying key changed after refresh!"
    );

    eprintln!("  Post-refresh signing works, group key unchanged");
    eprintln!("── Test D: signing_during_refresh ── PASSED");
}
