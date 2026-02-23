# Freenet Issues

## StreamId Collision in OrphanStreamRegistry Causes Silent PUT Failures

**Status**: Fix submitted as [PR #3202](https://github.com/freenet/freenet-core/pull/3202) — scopes registry by `(SocketAddr, StreamId)`
**Note**: This fix addresses cross-peer collisions but does NOT fix all PUT timeouts — see Issue #2 below
**Severity**: High — silently drops streaming PUT operations, causing 60-second client timeouts
**Affected code**: `crates/core/src/transport/peer_connection.rs`, `crates/core/src/operations/orphan_streams.rs`

### Summary

When two different peers send streaming PUT operations to the same node using coincidentally identical `StreamId` values, the `OrphanStreamRegistry`'s `claimed_streams` dedup map treats the second operation as a duplicate and silently drops it. The affected PUT never completes, and no error is propagated to the originator — the client just times out after 60 seconds.

### Root Cause

`StreamId` values are generated from a **thread-local counter** (`STREAM_ID_COUNTER` in `peer_connection.rs:133`), scoped per-thread on the **sender** side. Different nodes running on different threads can independently generate the same `StreamId`. When these streams arrive at a common receiver, they collide in the receiver's **global** `OrphanStreamRegistry`.

The `OrphanStreamRegistry.claimed_streams` map (`orphan_streams.rs:72`) is a `DashMap<StreamId, ()>` that tracks which stream IDs have been claimed, to deduplicate the embedded-metadata-in-fragment-#1 from the separate metadata message. But it does **not** scope entries by sender address or transaction — so a stream claimed by transaction A from peer X will cause transaction B from peer Y (with the same StreamId) to be rejected as `AlreadyClaimed`.

### Reproduction Evidence (from logs)

Network: 3 nodes (gateway `44vZ7`, node-1 `4edsb`, node-2 `23rCQ`).

1. **22:17:00.181** — Node-2 (`23rCQ`) pipes a streaming PUT to node-1 (`4edsb`) with `outbound_stream_id=2148083648`:
   ```
   peer=23rCQ: Starting piped stream forwarding to next hop
     tx=01KJ3PTSS... stream_id=2148083648 peer_addr=127.0.0.1:3102
   ```

2. **22:17:00.182** — Node-1 processes the `RequestStreaming`, calls `claim_or_wait(2148083648)` — succeeds (first claim):
   ```
   peer=4edsb: Processing PUT RequestStreaming
     tx=01KJ3PTSS... stream_id=2148083648 htl=8
   ```

3. **22:17:01.087** — Gateway (`44vZ7`) sends a *different* PUT to node-1 with the **same** `stream_id=2148083648`:
   ```
   peer=44vZ7: PUT request using operations-level streaming
     tx=01KJ3PTTR2... stream_id=2148083648 payload_size=265296
   ```

4. **22:17:01.113** — Node-1 receives the gateway's `RequestStreaming`, enters handler, calls `claim_or_wait(2148083648)` — returns **`AlreadyClaimed`** (collision with step 2). Handler returns `Err(OpNotPresent)`, which is silently treated as a benign duplicate:
   ```
   peer=4edsb: Processing PUT RequestStreaming
     tx=01KJ3PTTR2... stream_id=2148083648 htl=9
   ```

5. **22:17:01.113 – 22:17:09.408** — The duplicate embedded-metadata message retries 15 times with exponential backoff, hitting `OpNotAvailable::Running` each time (the transaction is stuck in `under_progress` with no handler left to clear it):
   ```
   ERROR: Error popping operation tx=01KJ3PTTR2... error=operation running
   (repeated 15 times)
   ```

6. **22:18:01.972** — Client disconnects after 60-second timeout. No `PutResponse` was ever received.

7. **22:18:05.087** — Garbage cleanup task times out the orphaned transaction:
   ```
   Transaction timed out tx=01KJ3PTTR2... elapsed_ms=64157 ttl_ms=60000
   ```

### Impact

- Streaming PUTs silently fail when StreamId collides — no error response to client
- More likely in busy networks with many concurrent PUT operations
- The transaction sits in `under_progress` for 60+ seconds, wasting resources
- No diagnostic output at INFO level — the failure is completely invisible

### Suggested Fix

Scope the `OrphanStreamRegistry` maps by `(SocketAddr, StreamId)` instead of just `StreamId`. Since stream IDs are only meaningful within a single peer-to-peer connection, the registry should track them per-connection:

```rust
// Before (global, collision-prone):
claimed_streams: DashMap<StreamId, ()>,
orphan_streams: DashMap<StreamId, (StreamHandle, Instant)>,
stream_waiters: DashMap<StreamId, oneshot::Sender<StreamHandle>>,

// After (per-connection, collision-free):
claimed_streams: DashMap<(SocketAddr, StreamId), ()>,
orphan_streams: DashMap<(SocketAddr, StreamId), (StreamHandle, Instant)>,
stream_waiters: DashMap<(SocketAddr, StreamId), oneshot::Sender<StreamHandle>>,
```

The `claim_or_wait` and `register_orphan` methods would need an additional `peer_addr: SocketAddr` parameter. This is already available at all call sites:
- `register_orphan` is called from `PeerConnection` which knows `self.remote_conn.remote_addr`
- `claim_or_wait` is called from PUT/GET handlers where `upstream_addr` is available

### Secondary Issue: Duplicate Metadata Dispatch

The embedded metadata in fragment #1 (fix #2757) causes every streaming operation to dispatch the `RequestStreaming`/`ResponseStreaming` message **twice**: once as the explicit metadata message, and once extracted from the first stream fragment. While the `claimed_streams` dedup handles this within a single transaction, the duplicate dispatch creates noisy ERROR logs ("Error popping operation... running") as the second handler retries 15 times over ~8 seconds before being dropped.

Consider either:
1. Not dispatching embedded metadata if the explicit metadata message has already been received (track at transport layer)
2. Demoting the "Error popping operation" log from ERROR to DEBUG for the `Running` case, since it's an expected transient condition during dedup

---

## Duplicate RequestStreaming Delivery Causes PUT Timeout (Separate from StreamId Collision)

**Status**: To be reported upstream
**Severity**: High — causes intermittent 60-second PUT timeouts even with the StreamId collision fix applied
**Affected code**: `crates/core/src/operations/put.rs`, message routing/forwarding logic

### Summary

A streaming PUT's `RequestStreaming` message is delivered **multiple times** to the same forwarding node within the same transaction. The first delivery starts piped stream forwarding and holds the operation state. Subsequent deliveries fail to pop the operation state ("operation running") and eventually corrupt the forwarding pipeline, preventing the response from reaching the originator.

### Root Cause

When a node receives a `RequestStreaming` and starts **piped stream forwarding**, it holds the operation in `under_progress` while assembling/piping. But the same `RequestStreaming` message arrives again at the same node (possibly via the embedded-metadata-in-fragment-#1 mechanism, or via duplicate network delivery). The duplicate message:

1. Tries to pop the operation state — fails with "operation running" (retries with exponential backoff)
2. Eventually succeeds when the original handler releases the state
3. But by then the piping context is stale/inconsistent
4. The response never propagates back to the originator

### Reproduction Evidence (from 2026-02-23 failure)

Transaction `01KJ3TNMD7HEGJEG9FN8V7YD81` — PUT for storefront contract `hDMcyk5JqK4fy1KQuBmxS4T2bxk86qBEtTAT1FcuEaK`:

1. **23:24:05.089** — Gateway (`44vZ7s`) sends `RequestStreaming` with `stream_id=2148383649` to peer `Dz2hqQ`

2. **23:24:05.091** — Peer `Dz2hqQ` receives it, starts piped forwarding:
   ```
   peer=Dz2hqQ: Processing PUT RequestStreaming tx=...D81 stream_id=2148383649 htl=9
   peer=Dz2hqQ: Starting piped stream forwarding inbound=2148383649 outbound=2147583648
   ```

3. **23:24:05.091–05.253** — A **duplicate** `RequestStreaming` for the same tx hits `Dz2hqQ` **6 times**:
   ```
   ERROR: Error popping operation tx=...D81 error=operation running (×6)
   ```

4. **23:24:05.263** — Duplicate eventually succeeds in popping the operation and processes the PUT locally

5. **23:24:05.414** — A **third** `RequestStreaming` arrives at `Dz2hqQ` for the same tx and stream_id

6. **23:25:09** — Transaction times out after 64 seconds. Gateway never received a response.

### Key Difference from Issue #1

Issue #1 (StreamId collision) is about **different transactions** from **different peers** colliding on the same StreamId. This issue is about the **same transaction** being delivered multiple times to the **same node**, causing operation state corruption. The StreamId collision fix (PR #3202) does not help here.

### Possible Fixes

1. **Idempotent RequestStreaming handler**: Track which `(Transaction, StreamId)` pairs have already been processed on a node. If a duplicate arrives, silently drop it instead of trying to pop/process the operation again.

2. **Prevent duplicate delivery**: Investigate why the same `RequestStreaming` message is delivered multiple times. This could be:
   - The embedded-metadata-in-fragment-#1 mechanism re-dispatching after the explicit message
   - A routing loop where the forwarded message comes back to the same node
   - Transport-level retransmission treating the metadata as unacknowledged

3. **Guard the pipe state**: Make the piping handler check whether forwarding is already in progress for this transaction before starting a second pipe.
