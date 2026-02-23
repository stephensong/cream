# CREAM - CURD Retail Exchange And Marketplace


A decentralized peer-to-peer raw dairy marketplace built on Freenet. Suppliers list products, customers browse and order, all coordinated with strong privacy through cryptographically signed contracts with no central server.

CURD (Completely Uncensorable Raw Dairy) is a form of e-cash implemented via the wondrous [Fedimint protocol](https://fedimint.org) allowing for totally private transactions within CREAM. CURD tokens can be exchanged for 
Bitcoin here (TODO: link forthcoming)


## Project Structure

```
cream/
├── common/                          # Shared domain models (cream-common)
├── contracts/
│   ├── directory-contract/          # Global supplier directory (WASM contract)
│   └── storefront-contract/         # Per-supplier inventory + orders (WASM contract)
├── delegates/
│   └── cream-delegate/              # Key management + signing service
├── ui/                              # Dioxus 0.7 web frontend (separate Cargo workspace)
└── Makefile.toml                    # cargo-make task definitions
```

## Tech Stack

- **Language**: Rust throughout
- **Contracts**: freenet-stdlib WASM contracts deployed to Freenet
- **UI**: Dioxus 0.7 (web + mobile targets, compiled to WASM)
- **Crypto**: ed25519-dalek for identity and signatures
- **Serialization**: serde_json
- **Network**: WebSocket connection to local Freenet node
- **Build**: cargo-make (`Makefile.toml`), `dx` CLI for UI

## Key Domain Types (in cream-common)

- `SupplierId` / `CustomerId` — ed25519 public key wrappers
- `Signed<T>` — generic signature envelope with verification
- `DirectoryState` — BTreeMap of supplier listings (LWW merge)
- `StorefrontState` — products + orders for one supplier
- `Product` — dairy product with category, price (CURD), quantity, expiry
- `Order` — customer purchase with deposit tier and monotonic status progression
- `GeoLocation` — lat/lon with Haversine distance; Australian postcode lookup

## Feature Flags

| Crate | Flag | Effect |
|-------|------|--------|
| cream-common | `dev` | **Disables signature verification** — never use in production |
| cream-common | `std` (default) | Enables chrono clock features |
| contracts | `contract` | Enables freenet-stdlib contract macro (required for WASM build) |
| contracts | `dev` | Propagates to cream-common, skips signature checks |
| ui | `web` (default) | Enables Dioxus web renderer |
| ui | `mobile` | Enables Dioxus mobile renderer (mutually exclusive with `web`) |
| ui | `use-node` | Enables real Freenet WebSocket connection; without it, runs offline |

## Build Commands

```bash
# Development (full fixture with live nodes)
cargo make fixture

# Development connected to local Freenet node
cargo make build-contracts-dev    # contracts with dev feature (no sig checks)
cargo make dev-connected          # UI dev server with use-node

# Production
cargo make build                  # builds everything (contracts + delegate + UI)

# Mobile development (requires Android Studio / Xcode)
CREAM_NODE_URL=ws://192.168.1.x:3001/v1/contract/command?encodingProtocol=native \
  cargo make dev-android          # Android emulator with remote node
cargo make dev-ios                # iOS simulator with remote node

# Mobile release builds
cargo make build-android
cargo make build-ios

# Testing & linting
cargo make test                   # cargo test --workspace
cargo make lint                   # fmt + clippy
cargo make check                  # cargo check --workspace
```

## Architecture Notes

- **Contracts** are compiled to `wasm32-unknown-unknown` and embedded as binary blobs in the UI
- **Conflict resolution**: Directory uses Last-Writer-Wins by timestamp; orders use monotonic status ordinals (Reserved → Paid → Fulfilled/Cancelled/Expired)
- **Sync protocol**: summarize → delta → merge (bandwidth-efficient)
- **Two-phase registration**: GET directory first, then PUT, to prevent race conditions between tabs
- **Delegate** holds private keys in memory and handles all signing operations
- **UI state**: `SharedState` (network data via signals) + `UserState` (local profile via context)

## Environment Variables

| Variable | Effect |
|----------|--------|
| `CREAM_NODE_URL` | Override the Freenet node WebSocket URL at compile time (default: `ws://localhost:3001/...`). Required for mobile builds pointing at a remote node. |

## Development Notes

- The UI is a **separate Cargo workspace** (excluded from root workspace) — run `dx` commands from `ui/`
- Contracts need the `contract` feature to compile as WASM — bare `cargo build` won't work for them
- The `dev` feature flag is critical during development to bypass ed25519 signature verification
- Freenet node must be running locally on port 3001 for `use-node` mode

## Mobile Support (Experimental)

Dioxus mobile renders the UI in a platform WebView, so the existing WASM code works unchanged. The `mobile` feature replaces the `web` renderer with `dioxus/mobile`.

**Prerequisites:**
- **Android**: Android Studio with SDK and NDK installed
- **iOS**: Xcode on macOS

**Known limitations:**
- Android support in Dioxus 0.7 is experimental (may crash on rotation, complex setup)
- iOS is more stable but still not production-ready for app stores
- The remote Freenet node must be network-accessible from the device
- No offline/caching support yet
