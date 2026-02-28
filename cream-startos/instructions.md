# CREAM Guardian

A FROST threshold signing guardian for the CREAM decentralized dairy marketplace.

## What is a Guardian?

Guardians collectively manage the root signing key for CURD e-cash using FROST threshold signatures (RFC 9591). In a 2-of-3 federation, any 2 guardians can authorize CURD operations (allocation, escrow release, etc.) but no single guardian can act alone.

## First-Time Setup

### 1. Configure Your Guardian

Set the following in the service config:

- **share-index**: Unique index for this guardian (1, 2, or 3 for a 3-node federation). Each guardian must have a different index.
- **port**: HTTP port (default: 3010). Usually leave as default.
- **max-signers**: Total guardians in the federation (e.g. 3).
- **min-signers**: Signing threshold (e.g. 2 for 2-of-3).

### 2. Run DKG (Distributed Key Generation)

On first start, guardians need to perform DKG to jointly generate the signing key. Set the **peers** field to a comma-separated list of the other guardians' URLs:

```
peers: http://guardian2.local:3010,http://guardian3.local:3010
```

All guardians must start with their peers configured at the same time. DKG runs automatically and saves keys to persistent storage.

### 3. Verify

After DKG completes, check the health endpoint — it should report `"ready": true`. Subsequent restarts will load keys from disk automatically (no need for peers on restart).

## Connecting to a Freenet Node

To enable contract monitoring, set **node-url** to the WebSocket URL of your Freenet node:

```
node-url: ws://localhost:3001/v1/contract/command?encodingProtocol=native
```

This is optional — the guardian functions for signing without it.

## Setting Up a 2-of-3 Federation

1. Install `cream-guardian` on 3 StartOS devices
2. Configure each with a unique `share-index` (1, 2, 3)
3. Set `max-signers: 3` and `min-signers: 2` on all three
4. Set `peers` on each to the other two guardians' addresses
5. Start all three — DKG will run automatically
6. Once all show `"ready": true`, the federation is operational
7. Remove the `peers` config — keys are now persisted and loaded on restart

## Backup

DKG keys are stored in the persistent data volume and included in StartOS backups. **Back up your guardian before any key rotation (refresh/redeal) operation.**
