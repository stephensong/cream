# CREAM — Infrequently Asked Questions

---

## What is a gateway in CREAM / Freenet?

A **gateway** is a **bootstrap node** — it's how new peers discover and join the Freenet network. Think of it like a Bitcoin seed node or a BitTorrent tracker.

### What it does

- Accepts "join ring" requests from new nodes that want to enter the network.
- Assigns the joining node a location on Freenet's small-world ring topology (a value between 0.0 and 1.0).
- Must have a publicly reachable IP address.

### What it does NOT do

- It has no special role in contract operations (PUT/GET/UPDATE/SUBSCRIBE).
- It doesn't route traffic differently from any other node.
- Once a node has joined via the gateway, the gateway is just another peer neighbor.

### Why do PUTs only work through the gateway in our tests?

That's likely a quirk of our tiny 3-node network that's only been alive for a few seconds. In a small, freshly-started network, contract routing may not work well from non-gateway nodes because the ring topology hasn't fully stabilised. In a mature production network with many peers, any node could accept a PUT and route it correctly.

### How would gateways work in production?

Yes, gateways would exist — multiple ones, run by different operators, with their addresses distributed in a hardcoded list (like Bitcoin's DNS seeds). Users would run a local Freenet node that joins via a gateway, then connects to CREAM's UI. After joining, the gateway is irrelevant — the user's local node handles all contract operations through the peer-to-peer mesh.

The Freenet team has noted that gateways are currently a potential single point of failure and plans to decentralise gateway management further.

---

## How does a user install CREAM?

CREAM is a decentralised app (dApp) that runs on top of Freenet. There is no central server to visit — the user runs a local Freenet node and the CREAM UI talks to it. There are two ways this could work: the production vision (Freenet-native packaging) and how it works today during development.

### Production vision: Freenet-native web app

In the fully realised Freenet ecosystem, CREAM would be distributed *through Freenet itself*:

1. **Install Freenet** — a one-liner installs the `freenet` binary:
   ```bash
   curl -fsSL https://freenet.org/install.sh | sh
   ```
   This downloads the binary for the user's OS/arch, verifies checksums, and installs to `~/.local/bin/`.

2. **Start the node as a system service:**
   ```bash
   freenet service install
   freenet service start
   ```
   This creates a systemd user service (Linux) or launchd agent (macOS) that runs automatically on login. The node joins the Freenet network via known gateway addresses and begins participating in the peer-to-peer mesh.

3. **Open CREAM in a browser** — the user navigates to:
   ```
   http://localhost:7509/v1/contract/web/{cream_contract_id}/
   ```
   The local Freenet node fetches the CREAM web app contract from the network, unpacks the UI assets (HTML, JS, WASM), and serves them directly. No external web server needed.

4. **Use CREAM** — the UI communicates with the directory and storefront contracts through the same local node's WebSocket API. All contract state lives on the Freenet network, replicated across peers.

In this model, someone (the CREAM developer) publishes the UI as a **WebApp contract** using `fdev publish`. The UI is packaged as a compressed tar.xz archive embedded in the contract state. Freenet's built-in HTTP server already handles Dioxus-specific asset path rewriting, suggesting this workflow is anticipated.

### What's not ready yet

This Freenet-native deployment path is not yet available:

- **`fdev publish --release` is disabled** — the developer tool currently bails with "Cannot publish contracts in the network yet" when targeting the real network. Local-mode publishing works.
- **No app discovery mechanism** — there is no "app store" or DNS-like naming system. Users would need to know CREAM's contract ID (a long hash) to access it. This will likely be solved by a directory or naming contract.
- **CREAM's UI packaging** — the Dioxus UI currently embeds contract WASM blobs at compile time and is served by its own dev server. For production, it would need to be packaged as a Freenet WebApp archive instead.

### How it works today (development mode)

During development, the process is manual:

1. Install Rust, `cargo-make`, and the `dx` (Dioxus) CLI.
2. Clone the CREAM repository.
3. Start a local Freenet node (or multi-node network): `cargo make reset-network`
4. Build contracts and run tests to populate data: `cargo make test-node`
5. Start the Dioxus dev server: `cd ui && dx serve --features use-node`
6. Open `http://localhost:8080` in a browser.

Or simply run `cargo make fixture` which does steps 3-5 in one command.

### The Freenet service

The `freenet` binary includes built-in service management:

| Command | Purpose |
|---------|---------|
| `freenet service install` | Install as a system service (auto-start on login) |
| `freenet service start` | Start the node |
| `freenet service stop` | Stop the node |
| `freenet service status` | Check if running |
| `freenet service logs` | Tail the node logs |
| `freenet update` | Self-update from GitHub releases |

The service also has **auto-update** built in: when the node detects a version mismatch with a gateway, it exits with a special code and the service wrapper automatically downloads the latest version before restarting.

---

## Can a new user use CREAM without installing a node?

**Short answer: Suppliers must run a node. Customers don't need to.**

CREAM has two types of user with different infrastructure requirements:

### Suppliers

Suppliers must install and run a full Freenet node. Their node is how they publish their storefront, list products, and receive orders. It also makes them part of the network infrastructure — the CREAM network *is* its suppliers. Installing a node is a one-liner (`curl -fsSL https://freenet.org/install.sh | sh`) and the service runs quietly in the background.

### Customers

Customers do not need to install a Freenet node. They use a mobile app (or browser) that connects to a local supplier's node directly. This is the natural model for a local raw dairy marketplace — you discover a farm near you, connect to their node, browse their products, and place orders.

The customer's app connects to the supplier's node via WebSocket, which gives them access to the full CREAM network through that node — they can browse the directory of all suppliers, not just the one they connected to. The supplier's node acts as their window into the network.

### How does a new customer find a supplier?

This is the bootstrap problem: a customer needs to connect to *some* node to see the directory, but they don't have a node and don't yet know any suppliers. Possible approaches include:

- **QR code at the farm gate or farmers market** — a supplier shares their node's URL, and scanning it opens the CREAM app pre-configured to connect.
- **Word of mouth / social media** — a supplier shares a link that launches the app pointed at their node.
- **A public web directory** — a simple website listing participating suppliers and their node addresses, searchable by location. This is a centralised convenience layer but doesn't compromise the decentralised marketplace itself.

### Privacy considerations

When a customer connects to a supplier's node, that supplier can see the customer's contract requests — which other suppliers they browse, what they order, and when they're online. In the context of a local dairy marketplace, where you're already showing up at someone's farm to collect raw milk, this is an acceptable trade-off. The customer trusts their local supplier in the same way they would trust any local shopkeeper.

### Becoming a supplier

If a customer later decides to become a supplier themselves, they would need to install a full Freenet node, register their storefront, and list their products. Their node then becomes part of the CREAM network and can serve other customers in turn.

---
