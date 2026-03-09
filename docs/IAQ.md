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
5. Start the Dioxus dev server: `cd ui && dx serve`
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

## What are the different types of participant in CREAM?

CREAM has three tiers of participant, distinguished by their presence on the Freenet network:

### Guests

A guest is someone browsing a supplier's storefront without participating in the network. They have no Freenet contract, no wallet, and pay no gas. They connect to a supplier's node via WebSocket (mobile app or browser), read the storefront — products, prices, opening hours, farm address — and that's it. They cannot place orders, hold deposits, or receive refunds.

Guests do not need to install a Freenet node. They connect to a supplier's node directly, which acts as their window into the network. This is the zero-commitment entry point: hear about a farm, open the app, browse the storefront.

### Users

A user is anyone who has created a **user contract** on the Freenet network. Users do not run their own Freenet node — they connect through a supplier's node. This contract is their identity and presence on the network — it makes them permanently contactable. The user contract holds:

- **Public key** — the user's ed25519 identity
- **Origin supplier** — the supplier who onboarded this user (immutable, set once at contract creation)
- **Current supplier** — the supplier node the user most recently connected through (updated on sticky login change)
- **Supplier logins** — a count of logins per supplier, tracking which nodes this user has connected through and how often
- **Cached suppliers** — up to three geographically proximate supplier endpoints, used as fallbacks if the current supplier is unreachable
- **Wallet state** — CURD balance and transaction history
- **Order references** — links to orders placed across various storefronts
- **Inbox** — a channel where other participants can deliver messages, refund tokens, or notifications

Because the user contract lives on hosting nodes near its key location (like any Freenet contract), anyone who knows the contract key can write to it — a supplier can push a refund directly to the user's contract rather than hoping they're subscribed to the storefront.

Creating a user contract costs gas (curds). This serves as sybil resistance — you can't spam the network with fake identities for free.

### Suppliers

A supplier is a user who *also* operates a storefront contract. They have both:

- A **user contract** (like any user) — their identity, wallet, inbox
- A **storefront contract** — their product listings, incoming orders, schedule

Suppliers must run a full Freenet node. Their node is how they publish their storefront, list products, and receive orders. It also makes them part of the network infrastructure — the CREAM network *is* its suppliers.

### The progression

The natural progression is:

1. **Guest** → Browse a storefront, see what's available. No commitment.
2. **User** → Decide to buy. Create a user contract (costs gas), fund a wallet, place orders. Always contactable for refunds and notifications.
3. **Supplier** → Decide to sell. Install a node, create a storefront contract, list products. Now part of the network infrastructure.

A guest becomes a user the moment they want to transact. A user becomes a supplier the moment they want to sell. Each step adds a contract to the network and a corresponding level of participation.

### Why users need contracts

The original design had customers as bare keypairs — just a public key that signs orders. This worked for placing orders but broke down for refunds: how do you deliver refund tokens to a customer who might not be subscribed to the storefront anymore? The answer was to encrypt tokens and stuff them into the storefront contract, hoping the customer checks back.

With user contracts, this problem disappears. Every transacting participant has a well-known location on the network. A supplier cancels an order and writes the refund directly to the user's contract. The user's app is subscribed to its own contract and receives the notification immediately — or whenever it next connects. No hoping, no polling, no encrypted blobs in shared state.

User contracts also enable:

- **Direct messaging** — supplier to user, user to supplier, outside the storefront context
- **Reputation** — order history and reviews can accumulate on the user contract
- **Identity persistence** — wallet state and preferences live on the network, not in browser sessionStorage
- **Cross-supplier history** — a user's contract tracks orders across all suppliers, not just the one they're currently browsing

### How does a guest find a supplier?

This is the bootstrap problem: a guest needs to connect to *some* node to see a storefront. Discovery happens through word of mouth and social media — a supplier tells people at the farmers market, posts on a local food group, or hands out a card.

- **QR code at the farm gate or farmers market** — a supplier shares their node's URL, and scanning it opens the CREAM app pre-configured to connect.
- **Word of mouth / social media** — a supplier shares a link that launches the app pointed at their node.

### How do users connect to the network?

Users don't run their own Freenet node. They connect to a supplier's node via WebSocket, and all their Freenet traffic routes through that node. The user contract records which supplier they connected through and tracks connection history.

When a guest upgrades to a user (by creating a user contract), the supplier they're connected to becomes their **origin supplier** — permanently recorded in the contract. This creates a tree structure: each user traces back to the supplier who onboarded them, each supplier traces back to whoever onboarded them, all the way to the network's root.

The **current supplier** field updates when the user connects through a different node. This is "sticky" — transient fallback connections don't change it. Only when the user has connected through an alternative supplier for several consecutive sessions does the current supplier update. This prevents brief outages from churning the field while still reflecting genuine migration.

### Supplier fallback and availability

Each user contract caches up to three geographically proximate supplier endpoints, populated from the directory at registration time and periodically refreshed. The user's app also caches these in localStorage.

If the current supplier is unreachable:

1. App tries cached supplier #2
2. Tries cached supplier #3
3. Falls back to rendezvous service lookup

This happens silently — the user doesn't need to know or care. The app reconnects through an alternative node and continues working. The user contract still exists on the network regardless of which node the user connects through.

When connected through a fallback supplier, the user can potentially detect manipulation by their primary supplier. If contract state looks different through the fallback, something was wrong with the primary. This provides an implicit consistency check for free.

### Security considerations

The supplier node is the user's only connection to the Freenet network. This means the supplier can:

- **Observe traffic** — see which contracts the user subscribes to, what orders they place, when they're online.
- **Theoretically serve fake state** — present manipulated contract data for other suppliers' storefronts.
- **Censor selectively** — drop subscription notifications, delay order updates, hide competing suppliers.

These risks are mitigated by several factors:

- **The guardian federation protects money regardless.** Even if a supplier manipulates Freenet contract state, deposits are held in escrow by the FROST guardian federation. The supplier can't steal funds by tampering with the coordination layer.
- **Fallback suppliers enable cross-checking.** If a user occasionally connects through a different node and sees different state, the inconsistency is detectable.
- **Reputation is at stake.** Supplier reputation (see below) is publicly computable. Misbehaviour leads to user migration, which is visible in the login history across user contracts.
- **The real-world relationship.** In a local dairy marketplace, you physically visit the farm. The trust relationship already exists — trusting a supplier not to tamper with your network view is comparable to trusting them not to sell you bad milk.

### Privacy considerations

When a guest connects to a supplier's node, that supplier can see the guest's requests. Once a guest becomes a user, their traffic still routes through a supplier's node, but their identity is now network-persistent — their user contract exists on the network regardless of which node they happen to be connected through.

The origin supplier link in the user contract is publicly readable, which means anyone can trace the tree structure. This reveals who onboarded whom — a social graph of the network's growth. In the context of a local dairy community this is acceptable (these relationships are already visible in the real world), but it's worth noting for privacy-sensitive deployments.

---

## How does a new guest find a local supplier?

A new guest needs to connect to a supplier's Freenet node to browse their storefront. But due to the legal sensitivities around raw dairy, there is no public web directory of suppliers. Discovery happens through word of mouth and social media — a supplier tells people at the farmers market, posts on a local food group, or hands out a card.

For this to work, each supplier needs a simple, memorable name that guests can type into the CREAM mobile app.

### The rendezvous service

CREAM uses a lightweight **rendezvous service** — a simple lookup that maps memorable names to node addresses. When a supplier registers, they pick a short name like `garys-farm`. When a guest types that name into the CREAM app, the app asks the rendezvous service "where is garys-farm?", gets back the supplier's IP and port, and connects directly to the supplier's node via WebSocket.

The rendezvous service is only involved in that initial lookup. All actual marketplace traffic flows directly between the guest's app and the supplier's node.

### What the rendezvous service handles

- **Registration**: A supplier picks a name and associates it with their node address. Their CREAM node handles this automatically: `cream register garys-farm`.
- **Dynamic IPs**: Home internet connections change IP addresses. The supplier's node periodically pings the rendezvous service with its current address (a heartbeat every few minutes, like a dynamic DNS client).
- **Lookup**: A simple HTTPS GET that returns a node address. Extremely lightweight, easily cacheable.

### What the guest sees

The guest experience is:

1. Hear about a farm through word of mouth or social media.
2. Download the CREAM mobile app.
3. Type in the supplier's name — e.g., `garys-farm`.
4. See **only that supplier's storefront** — products, prices, opening hours.
5. No directory, no browsing other suppliers, no ordering.

Restricting guests to a single supplier's storefront solves two problems at once: it protects the privacy of the broader network (guests can't enumerate all suppliers), and it simplifies the onboarding experience. When a guest decides to buy, they create a user contract and become a user — at which point they can place orders and receive refunds.

### Centralisation trade-offs

The rendezvous service is a centralised component in an otherwise decentralised system. This means:

- **If it goes down**, new guests can't discover suppliers. But existing users who already have a saved address can still connect directly.
- **If it's seized or pressured**, the operator could be forced to take down listings or hand over the mapping data — which reveals which IP addresses are running CREAM nodes.
- **If it's compromised**, an attacker could redirect customers to malicious nodes.

### Why the centralisation is acceptable

- **It only stores names and IP addresses.** No marketplace data, no orders, no user information. The actual marketplace is fully decentralised on Freenet. Seizing the rendezvous service gives you a list of supplier IPs, but those IPs are already being shared openly via word of mouth.
- **It's trivially replaceable.** If one rendezvous service goes down, another can be stood up in minutes. The CREAM app could ship with a list of fallback rendezvous URLs (like Bitcoin ships with multiple DNS seeds). Community members could run their own.
- **It's only needed once per supplier.** Once a guest has connected to a supplier and saved their address, the rendezvous service is no longer needed for that relationship. The app caches resolved addresses locally.
- **It could be replicated.** Multiple independent operators could run rendezvous services, each with overlapping or partial directories. No single operator needs the complete list.

### Future evolution

Eventually, the name-to-address mapping could itself be a Freenet contract — a directory of supplier endpoints stored on the network. But this creates a chicken-and-egg problem: you need a node to read the contract, and you need the directory to find a node. A lightweight centralised rendezvous is the right starting point, with decentralisation as a future evolution.

---

## How does supplier reputation work?

Supplier reputation is derived from observable, publicly verifiable data in Freenet contracts. There is no central reputation authority — anyone can compute any supplier's reputation score from the contracts on the network.

### Reputation signals

| Signal | Source | What it measures |
|--------|--------|-----------------|
| **Uptime / availability** | Subscription failures, ping timeouts observed by other nodes | Is the node reliably reachable? |
| **Order fulfilment rate** | Storefront contract order statuses | Ratio of Fulfilled to Cancelled/Expired orders |
| **Refund timeliness** | User contract inboxes, escrow release timestamps | When orders are cancelled, how quickly do refunds appear? |
| **Onboarding count** | User contracts with this supplier as origin | How many users has this supplier brought into the network? |
| **Active users** | User contracts with this supplier as current | How many users currently connect through this node? |
| **User retention** | Origin vs. current supplier across user contracts | Of those onboarded, how many still connect through this supplier vs. having migrated away? |
| **User attraction** | User contracts where origin ≠ current and current = this supplier | Users who were onboarded elsewhere but chose to switch to this supplier |

### Why retention and attraction matter

Retention and attraction are the strongest signals. A supplier who onboards 100 users but retains only 20 has a problem — 80 people chose to leave. A supplier who onboards 30 and retains 28 is doing something right. And a supplier who *attracts* users from other suppliers — people who actively chose to switch — is demonstrably better than the alternative they left.

These metrics are self-correcting. A supplier who censors traffic, serves manipulated state, or runs an unreliable node will see users migrate away. The migration is visible in user contracts (current supplier changes), and the resulting drop in retention feeds directly into the reputation score.

### How login tracking works

Each user contract maintains a `supplier_logins` map — a count of logins per supplier. Every time a user's app connects to the network and authenticates, the login is recorded against the supplier node they connected through. This data is public (it's in the user contract on the network), so anyone can aggregate it.

The **current supplier** field in the user contract is "sticky" — it only updates when the user has connected through a different supplier for several consecutive sessions. Transient fallback connections (when the primary is briefly unreachable) don't count. This prevents short outages from unfairly penalising a supplier's retention score.

### Invitation-based onboarding

Suppliers can issue **invitations** to encourage new users to join the network. An invitation is a signed token containing the supplier's name, node address, and an optional expiry. The supplier shares it via QR code at the farm gate, a link on social media, or a text message to a regular customer.

When a guest receives an invitation:

1. The CREAM app connects to the supplier's node (as for any guest).
2. When the guest upgrades to a user (creates a user contract), the invite token is recorded as provenance — the user contract's origin supplier is set to the inviting supplier.
3. The supplier's onboarding count increments.

The invitation provides verifiable provenance — the signed token proves which supplier onboarded which user. Without it, anyone could create a user contract claiming affiliation with a popular supplier to inflate their reputation.

### Incentive alignment

The reputation system creates a virtuous cycle:

- **Suppliers want high reputation** because it attracts more users (customers).
- **High reputation requires** reliable uptime, honest order fulfilment, timely refunds, and active onboarding.
- **Onboarding more users** directly boosts reputation, so suppliers are motivated to issue invitations and grow the network.
- **Retaining users** matters more than just onboarding them — a supplier who onboards aggressively but provides poor service will see users migrate away, hurting their retention score.
- **Gas revenue** — if suppliers earn a fraction of gas fees from operations routed through their node, onboarding and retaining users is directly profitable. Users whose contract operations (placing orders, wallet transactions) route through the supplier generate ongoing gas fees.

This aligns the supplier's economic interest with keeping their node reliable, their service honest, and their users happy.

---

## What currency does CREAM use?

CREAM uses a single currency: **curds**. Every price, deposit, payment, refund, and gas fee in the CREAM network is denominated in curds. There are no currency selectors, no exchange rate feeds, no conversion layers.

### What is a curd?

A curd is a unit of e-cash backed 1:1 by a Bitcoin satoshi. Each curd is held by CREAM's **guardian federation** — a set of trusted community nodes that collectively control the root authority via FROST threshold signatures (see "How does the guardian federation work?" below). By holding curds, you are holding Bitcoin — just in a more private and convenient form.

Curds are managed through CREAM's double-entry ledger: user contracts track balances, the root user (controlled by the guardian federation) holds escrow and issues allocations. The guardian federation cannot unilaterally trace or block individual transactions — a threshold quorum is required for any root operation, and the ledger is publicly auditable on the Freenet network.

### How do you get curds?

Through the guardian federation's Lightning gateway:

- **Buy curds (peg-in)**: Send Bitcoin satoshis via Lightning → the guardian federation credits equivalent curds to your CREAM wallet (a debit from root's float, a credit to your user contract).
- **Sell curds (peg-out)**: Request redemption → the guardian federation threshold-signs a Lightning payment to your wallet, debiting your user contract.

Any Bitcoin Lightning wallet (Phoenix, Muun, Wallet of Satoshi, etc.) can send sats to the CREAM federation and receive curds in return.

### Why curds rather than raw Bitcoin?

Curds provide three things that raw Bitcoin (on-chain or Lightning) doesn't:

1. **Privacy**: Chaumian blind signatures mean the federation cannot trace spending. Bitcoin on-chain transactions are fully traceable; even Lightning has routing metadata. Curds are genuinely private — the federation knows the total amount issued but not who holds what or who paid whom.
2. **Speed and cost**: Curd transfers within CREAM are instant and free — they're just e-cash token exchanges between participants. No on-chain fees, no Lightning routing fees, no channel capacity constraints.
3. **Offline capability**: E-cash tokens are bearer instruments. A payment can be made by handing over the token bytes — no network round-trip to the federation is needed at the moment of payment. The recipient can verify and reissue the tokens later when they're online.

### Curds as network gas

Curds also serve as the **fee currency for the CREAM network**. Every network operation (creating a user contract, listing a product, placing an order, updating a storefront) costs a small amount of curds. This prevents spam and abuse while funding the infrastructure — guardian nodes and Freenet node operators.

The exact fee schedule and distribution mechanism are yet to be determined, but the fees will be very low — fractions of a cent per operation. The marketplace should feel free to use; fees exist to prevent abuse, not to extract revenue.

### The trust trade-off

The cost of curds' privacy and speed is trust in the guardian federation. A threshold of guardian nodes must remain honest for the system to work. If enough guardians collude, they could theoretically mint unbacked curds or refuse to honour redemptions.

For a local dairy marketplace, this is an acceptable model — especially since the federation is run by the community itself. The guardians are trusted community members (not necessarily suppliers). Their reputation, their livelihoods, and their community relationships are at stake. And the amounts involved (dairy product deposits and payments) are small enough that the risk-reward for guardian misbehaviour doesn't make sense.

Since every curd is backed 1:1 by a satoshi, holding curds is economically equivalent to holding Bitcoin. The federation is a privacy layer and convenience layer, not a speculative instrument.

---

## How does the guardian federation work?

CREAM has its own native guardian federation — a small set of trusted nodes that collectively control the root authority (source of all CURD, escrow for deposits, toll collection).

### The root user

There is a **root user** — the system account that represents the guardian federation. Root is the source of all CURD (initial float), holds deposits in escrow, and collects message tolls. Root's user contract lives on Freenet like any other user contract. Its balance is auditable: total CURD issued minus allocations minus refunds.

The root user is not a single person — it's a collective identity controlled by the guardian federation via threshold cryptography. No single guardian holds the root private key. A quorum of guardians (e.g. 2-of-3, 3-of-5) must cooperate to authorise any debit from root's ledger.

### Guardian nodes

A guardian node is a Freenet node operated by a trusted community member. Guardians are **not** implicitly suppliers — they are infrastructure operators. A guardian may also choose to be a supplier, but the roles are independent.

Each guardian runs three services:
- A **CREAM/Freenet node** — participates in the peer-to-peer network, hosts contracts, and processes signing rounds
- A **Bitcoin full node** — validates the blockchain independently, ensuring the federation's Bitcoin holdings are verifiable without trusting external services
- A **Lightning node** — manages payment channels for peg-in/peg-out (buying and selling CURD for sats), enabling fast settlement without on-chain transactions for every trade

Guardian responsibilities:
- **Hold a key share** of the root signing key (via FROST threshold signatures)
- **Participate in signing rounds** when root debits are requested (escrow releases, initial allocations, refunds)
- **Maintain liveness** — the network needs a quorum of guardians available at all times
- **Run DKG (distributed key generation)** when the guardian set changes
- **Operate Bitcoin and Lightning infrastructure** — keep nodes synced, channels funded, and available for peg-in/peg-out settlement

### Bootstrapping ceremony

The network bootstraps incrementally:

1. **Genesis**: The root user starts as a single entity — the first guardian. They hold the full root key (FROST supports 1-of-1 as a degenerate case). The root public key is established at this point and never changes.
2. **Second guardian joins**: The root user invites guardian #2. They run a DKG protocol together, resharing the root key into a 2-of-2 threshold scheme. The public key stays the same — only the key shares change.
3. **Third guardian joins**: Another DKG round, resharing into a 2-of-3 threshold scheme. The network is now live — any single guardian can go down without disrupting operations.
4. **Growth**: Additional guardians can join via further DKG resharing rounds (3-of-5, 4-of-7, etc.), always maintaining a fault-tolerant quorum.

The critical property: the root public key is fixed at genesis and never changes. Every contract that references root's identity continues to work through every guardian set transition. Key resharing changes who holds the shares, not the public key they produce.

### FROST threshold signatures

CREAM uses **FROST (Flexible Round-Optimized Schnorr Threshold signatures)** for guardian key management. FROST allows the guardian set to collectively produce a single ed25519 signature without any party ever reconstructing the full private key.

Why FROST over multi-sig:
- **Single signature, single public key** — the user contract sees one owner key and one signature, just like any other user. The contract validation logic doesn't need to know about guardians at all.
- **Key shares never leave guardians** — no single guardian can sign alone, no reconstructed secret exists anywhere.
- **Compatible with ed25519** — CREAM already uses ed25519 throughout. The `frost-ed25519` crate provides a working implementation.
- **Key resharing** — guardians can be added or removed without changing the public key, enabling smooth transitions.

### The user contract validation rule

User contracts enforce a simple rule: **anyone can credit, only the owner can debit**.

- **Credit entries** (receiving CURD) can be appended by anyone — no signature required from the contract owner. This enables direct peer-to-peer transfers: a customer can credit a supplier's contract for a deposit without the supplier's cooperation.
- **Debit entries** (spending CURD) must be signed by the contract owner. For regular users, this is their personal ed25519 key. For the root user, this is the guardian federation's threshold key — requiring a quorum to authorise.

This validation rule is the foundation of the entire CURD economy:
- **Initial allocation**: Root debits itself (threshold-signed), credits the new user (no signature needed on the user's contract)
- **Order deposit**: Customer debits themselves (self-signed), credits root's escrow (no signature needed on root's contract)
- **Escrow release**: Root debits itself (threshold-signed), credits the supplier (no signature needed on the supplier's contract)
- **Refund**: Root debits itself (threshold-signed), credits the customer back (no signature needed)
- **Message toll**: Customer debits themselves (self-signed), credits root (no signature needed)

Guardian involvement is only required for root debits — everything else flows freely without threshold signing coordination.

### The rendezvous service's role

The rendezvous service (Cloudflare Workers, edge-routed, globally distributed) expands from simple name→address lookup to become the guardian coordination layer — analogous to a WebRTC signaling server:

- **Guardian discovery**: Publishes the current guardian set and the root contract key so all participants can find them.
- **DKG coordination**: When bootstrapping or rotating the guardian set, guardians exchange key generation messages through the rendezvous service. It relays but never holds key material.
- **Signing request relay**: When a client needs a root debit (escrow release, refund, initial allocation), it posts the request to the rendezvous service, which fans it out to guardians for threshold signing. The signed result is posted back to the Freenet contract.
- **Health and failover**: Guardian heartbeats, liveness detection, and leadership coordination for signing rounds.

The rendezvous service remains untrusted infrastructure — it routes messages but never holds keys or funds. If it goes down, existing operations continue (Freenet contracts are self-sustaining); only new guardian coordination and discovery are affected. Multiple rendezvous instances can run for redundancy.

### Lightning settlement

CURD is backed by Bitcoin satoshis. The guardian federation collectively controls a Lightning node (or threshold-controls channel keys):

- **Buy CURD (peg-in)**: Send sats via Lightning → guardian federation mints equivalent CURD (credit to user's ledger, debit from root's float)
- **Sell CURD (peg-out)**: User requests redemption → guardian federation threshold-signs a Lightning payment, sends sats to user's Lightning wallet

The guardian coordination happens through FROST signing rounds relayed via the rendezvous service.

### Future: Fedimint wallet integration

CREAM's guardian architecture is designed to be compatible with Fedimint. If a community already runs a Fedimint federation, their existing threshold key could serve as the root key, and Fedimint's consensus engine would handle coordination. The CREAM user contract interface would be identical — same owner public key, same signature validation.

This is a post-launch consideration. CREAM's native FROST-based guardian federation provides all the same guarantees — threshold custody, escrow, and Lightning settlement — without the additional complexity and dependency of a full Fedimint deployment. Fedimint integration may be offered as an optional wallet backend for communities that prefer it.

### Trust model

- **Threshold honesty**: A quorum of guardians (e.g. 2-of-3) must be honest. If enough guardians collude, they could mint unbacked CURD or refuse refunds.
- **Acceptable for local communities**: In a dairy marketplace, guardians are community members with real-world reputations. The amounts involved are small. The risk-reward for misbehaviour doesn't make sense.
- **Auditable**: Root's ledger is public on Freenet. Anyone can verify that total CURD issued matches total sats locked. Any discrepancy is immediately visible.
- **Recoverable**: If a guardian goes down, the remaining quorum continues operating. If a guardian is compromised, the set can be reshared to exclude them without changing the root public key.

---

## What automated testing suites are deployed with the CREAM dApp to ensure highest quality control?

CREAM uses a three-tier testing strategy: node integration tests (Rust), browser E2E tests (Playwright), and a development fixture that ties everything together. There are no Rust unit tests at present — all testing is integration or end-to-end.

### Node integration tests

**Location**: `tests/node-integration/`
**Run**: `cargo make test-node`

These are Rust tests that exercise real Freenet contracts on a live multi-node network. A single cumulative test (`cumulative_node_tests`) runs 7 sequential steps, each building on the state left by previous steps:

1. Directory subscribe → update → notification (cross-node)
2. Storefront subscribe → add product → notification
3. GET with subscribe flag vs explicit Subscribe
4. Product count increments for subscriber (0 → 1 → 2)
5. Full harness: 3 suppliers with products, multi-customer subscriptions
6. Order expiry across deposit tiers (backdated orders)
7. Opening hours schedule update → subscriber notification

The test harness (`harness.rs`) manages 5 participants distributed across nodes:
- **Gateway (port 3001)**: Gary (supplier), Iris (supplier), Alice (customer)
- **Node 2 (port 3003)**: Emma (supplier), Bob (customer)

This exercises cross-node contract propagation — the hardest thing to get right in a decentralised system. Identity derivation is deterministic (name + lowercase password), producing the same ed25519 keys as the UI, so harness data is directly usable by E2E tests and manual testing.

The `reset-network` task handles all node lifecycle: kill existing processes, wipe state, generate transport keypairs, start a 3-node network with proper gateway configuration.

### Playwright E2E tests

**Location**: `tests/e2e/`
**Run**: `cargo make test-e2e` (full pipeline) or `cd tests/e2e && npx playwright test` (tests only)

Browser-based tests using Playwright (Chromium) that exercise the full CREAM UI against a live Freenet network. Tests run sequentially (`workers: 1`) because they share network state from the node integration fixture. There are currently 13 test suites:

| # | Test | What it verifies |
|---|------|-----------------|
| 01 | Setup flow | Supplier and customer registration, header/nav rendering |
| 02 | Directory view | Shows 3 harness suppliers, search/filter by name |
| 03 | Supplier dashboard | Harness products visible, Add Product form toggle |
| 04 | Add product | Create product via form, verify it appears in list |
| 05 | View storefront | Customer sees Order buttons; supplier sees own-storefront note |
| 06 | Cross-tab updates | Gary adds product → Emma sees updated count in directory |
| 07 | Login persistence | Page reload preserves session; logout clears it |
| 08 | Place order | Emma orders from Gary; Gary sees incoming order in dashboard |
| 09 | Wallet balance | Deposit deducted from customer, credited to supplier |
| 10 | Returning user | Log out → re-enter name → fields auto-fill from directory |
| 11 | Order decrements quantity | Order reduces available quantity on both sides |
| 12 | Customer rendezvous | Supplier → rendezvous registration → customer lookup → auto-connect |
| 13 | Schedule editor | Returning supplier opens Edit Hours, modifies schedule, saves |

**Helpers** (`tests/e2e/helpers/`):
- `setup-flow.ts` — `completeSetup()` handles the full registration wizard (name, postcode, locality, supplier checkbox, description). In dev mode the password is derived automatically.
- `wait-for-app.ts` — `waitForAppLoad()`, `waitForConnected()`, `waitForSupplierCount()` — wait for WASM compilation, Freenet connection, and directory sync.

### The fixture pipeline

**Run**: `cargo make fixture` (development) or `cargo make test-e2e` (CI-style)

The `fixture` task runs the entire stack in order:

1. `kill-stale` — kill leftover `dx serve` and `cargo-make` processes from previous runs
2. `build-contracts-dev` — compile directory and storefront contracts to WASM with `dev` feature (no signature checks)
3. `reset-network` — stop Freenet, wipe state, start 3-node network
4. `test-node` — run all 7 node integration steps (populates Gary/Emma/Iris with products and orders)
5. `restart-rendezvous` — start the supplier lookup service on port 8787
6. `tailwind-build` — compile CSS
7. `dx serve` — start the Dioxus dev server connected to the live network

After the fixture completes, the UI at `http://localhost:8080` shows a fully populated marketplace. Typing "gary" in the setup screen auto-fills from the directory — the same data that the E2E tests exercise.

For CI-style runs, `test-e2e` builds the UI statically with `dx build`, serves it on port 8080, runs Playwright, and cleans up. The rendezvous E2E tests have a separate pipeline (`test-e2e-rendezvous`) that also starts the Wrangler-based rendezvous service.

### Test data flow

The key design principle is that **all test layers share the same identity derivation and fixture data**:

- Node integration tests create suppliers Gary, Emma, Iris with deterministic keys (name + lowercase password).
- E2E tests log in as those same identities and see the products/orders/schedules created by the node tests.
- The `fixture` task makes the same data available for manual testing in the browser.

This means a bug caught by an E2E test can always be reproduced at the node integration level, and vice versa.

---

## How does the test suite in CREAM actually work?

CREAM's test suite is **cumulative and sequential**. Every test assumes all previous tests have passed, and the datastore reflects the mutations they introduced. This is a deliberate design choice — not a limitation.

### The principle

The network is started once. The node integration tests populate it with fixture data (suppliers, products, orders, schedules). The E2E tests then run against that same network, in order, each building on the state left by its predecessors. There is no teardown, no reset, no isolation between tests. State accumulates.

This mirrors how the real CREAM network works: data persists, contracts accumulate state, and every participant sees the history of everything that came before. Testing in this mode catches real bugs that per-test isolation would hide.

### Fail-fast and predictable state

The Playwright config enforces strict sequential execution:

```typescript
maxFailures: 1,    // Stop at the first failure
workers: 1,        // No parallelism
fullyParallel: false,
retries: 0,        // No retries — a failure is a failure
```

Files are named with numeric prefixes (`01-setup-flow`, `02-directory-view`, ...) so Playwright runs them in alphabetical order, which is execution order. When a test fails, the suite stops immediately. The datastore at that point contains exactly the cumulative state of all prior tests — no more, no less. This makes debugging deterministic: you know what happened before the failure.

### Cumulative state map

Each test documents the state it expects via inline comments. Here's the state progression:

| After test | Gary's products | Gary's orders | Notes |
|-----------|----------------|--------------|-------|
| Harness | 4 | 3 expired | Node integration steps 1–7 |
| 04 – Add Product | 5 | 3 expired | + "Organic Goat Cheese" |
| 06 – Cross-Tab | 6 | 3 expired | + "Cross-Tab Test Milk" |
| 08 – Place Order | 6 | 4 (3 exp + 1 res) | Emma orders 2 units |
| 09 – Wallet | 6 | 5 (3 exp + 2 res) | Emma orders 2 more |
| 11 – Qty Decrement | 6 | 6 (3 exp + 3 res) | Emma orders 2 more |
| 13 – Schedule | 6 | 6 | Sunday hours added |

Tests 01, 02, 03, 05, 07, 10, 12 use fresh identities or read-only assertions and do not mutate Gary's storefront.

### Writing cumulative tests

When adding a new E2E test:

1. **Know the state.** Read the table above and the preceding tests. What products exist? What orders? What's the wallet balance?
2. **Assert exact counts.** Use `toBe(N)` not `toBeGreaterThanOrEqual(N)`. Exact assertions catch unexpected mutations from earlier tests.
3. **Document your mutations.** Add an inline comment like `// Cumulative state: Gary has 6 products (4 harness + test-04 + test-06)` and update the table above.
4. **Use `.first()` or text filters** when selecting elements whose count may grow. Don't assume an element is unique if prior tests could have created siblings.

### Node integration tests follow the same model

The 7 node integration steps (`tests/node-integration/tests/node_tests.rs`) run in a single Rust test function. Each step builds on the previous: step 1 subscribes to the directory, step 2 adds products, step 5 runs the full harness, step 6 backdates orders to test expiry, step 7 updates the schedule. State flows forward, never backward.

### The rendezvous exception

Test 12 (`customer-rendezvous`) runs in a separate Playwright project because it requires the Wrangler rendezvous service running. It uses `>= 4` for product counts since it may run on a fresh network or after the cumulative suite — its Playwright project configuration (`testMatch: /12-customer-rendezvous/`) excludes it from the main sequential run.

---

## How do deposits and refunds work?

CREAM uses a **ledger-based escrow** model. When a user places an order, the deposit amount is transferred from the user's CURD balance to the root user's escrow account via the double-entry ledger. The guardian federation (via FROST threshold signatures) controls the root account and releases escrow funds based on order outcomes.

### The deposit flow

When a user places an order, they transfer curds to escrow as the deposit. The deposit amount is determined by the order's deposit tier (e.g. 10%, 25%, 50%).

1. **User places order**: The user's app debits their user contract (self-signed) and credits the root user's escrow balance. Simultaneously, the order is posted to the storefront contract as a state update.
2. **Contract validates**: The storefront contract's `verify_delta()` checks the state transition is legal (new order, valid signature, correct deposit tier). The user contract validates the debit signature.
3. **Escrow is held by root**: The deposit now sits in root's escrow ledger, controlled by the guardian federation. Neither the user nor the supplier can unilaterally access it.

At this point, the deposit is held in escrow by the guardian federation. What happens next depends on how the order resolves.

### Terminal states and fund flows

An order can reach three terminal states, each with a different fund flow:

#### Fulfilled (happy path)

The user collects their product. The supplier marks the order as `Fulfilled` via a contract update. The user pays the remaining balance (total price minus deposit) in person or via a second CURD transfer. The guardian federation threshold-signs the escrow release — debiting root's escrow and crediting the supplier's user contract. Everyone is happy.

#### Cancelled (supplier-initiated)

The supplier cannot or chooses not to fulfil the order. The deposit must be returned to the user.

1. **Supplier updates the storefront contract** transitioning the order to `Cancelled`.
2. **Guardian federation releases escrow** — the guardians threshold-sign a root debit that credits the deposit back to the user's user contract.
3. **User receives notification** — the user's app is subscribed to its own contract and sees the refund immediately.

The user doesn't need to trust the supplier — the guardian federation held the funds from the start and releases them back automatically on cancellation.

#### Expired (user no-show)

The user reserved product, the supplier held it aside, and the user never collected within the reservation window. The supplier is owed the deposit as compensation for the opportunity cost of holding inventory.

On the Freenet layer, the contract transitions to `Expired` (the supplier's node runs the expiry check, already implemented in `node_api.rs`) and the held product is released back to available inventory. The guardian federation threshold-signs the escrow release to the supplier's user contract. No refund is posted.

### How Freenet contract notifications work

When a user subscribes to a storefront contract (or their own user contract), the request routes through Freenet's small-world network to the **hosting nodes** — the handful of nodes whose location on the ring is nearest to the contract's location (derived deterministically from the contract key hash). Those hosting nodes store the full contract state and maintain a subscriber list.

When someone posts an update (a cancellation, a refund delivery to a user contract, a fulfilment confirmation, or any other state change):

1. The update routes to the hosting nodes.
2. They validate it via `verify_delta()` and merge it via `update_state()`.
3. They push an `UpdateNotification` to every subscriber.

This is **point-to-point routing**, not flooding. Each hop follows the small-world graph toward the target location. Path length is O(log n) hops. Only explicitly subscribed nodes receive notifications.

In our codebase, this is exactly what `node_comms()` in `node_api.rs` already does — subscribe to contracts and reactively update `SharedState` when notifications arrive. The same mechanism that shows order status changes in the UI today would deliver refund tokens to a user's contract tomorrow.

### What the Freenet contracts enforce vs. what they don't

**The storefront contract enforces:**
- Valid state transitions (Reserved → Paid → Fulfilled/Cancelled/Expired) — the `verify_delta()` function rejects illegal transitions
- Correct signatures — only the supplier can update their own storefront, only the user can place an order
- Monotonic status progression — an order can't go backwards (Fulfilled → Reserved is rejected)

**The user contract enforces:**
- Only the user and authorised parties (suppliers they've ordered from) can write to the contract
- Inbox messages are append-only — a supplier can deliver a refund but can't delete previous entries

**Freenet contracts enforce coordination; the guardian federation enforces fund movement.** The user contract ledger tracks balances, and the root user (controlled by the FROST guardian federation) holds escrow. This eliminates the trust gap: the guardian federation holds deposits from the moment an order is placed, so supplier insolvency is not a risk and cancellation refunds are automatic.

### Why the trust model works

With the guardian-based escrow model, trust requirements are minimal:

- **Fulfilment**: No trust needed. User pays, gets product. Supplier gets paid. Both parties are satisfied before the transaction completes.
- **Expiry**: No trust needed. Guardian federation releases locked deposit to supplier after timeout.
- **Cancellation**: No trust needed. Guardian federation releases locked deposit back to user automatically. The supplier doesn't need to have funds — the federation already has them.

---

## How does escrow work in CREAM?

CREAM's escrow is built directly into the native ledger. There is no separate escrow system — the same Freenet contracts and FROST guardian federation that manage CURD balances also manage escrow.

### How it works

When a user places an order, their deposit is debited from their user contract and credited to the root user's escrow balance. The root user is controlled by the FROST guardian federation — no single guardian can release or redirect escrow funds.

The storefront contract tracks the order's status progression (Reserved → Paid → Fulfilled/Cancelled/Expired). When a terminal state is reached, the guardian federation threshold-signs the appropriate escrow release:

- **Fulfilled**: Root debits escrow, credits supplier
- **Cancelled**: Root debits escrow, credits user (refund)
- **Expired**: Root debits escrow, credits supplier (compensation)

### How the terminal states work

#### Fulfilled (happy path)

| Step | What happens |
|------|-------------|
| 1 | Supplier updates order status to `Fulfilled` in the storefront contract |
| 2 | Guardian federation threshold-signs escrow release — root debits escrow, credits supplier's user contract |

#### Cancelled (supplier-initiated)

| Step | What happens |
|------|-------------|
| 1 | Supplier updates order status to `Cancelled` in storefront |
| 2 | Guardian federation threshold-signs escrow refund — root debits escrow, credits user's user contract |

The user doesn't need to trust the supplier — the guardian federation held the funds from the moment the order was placed. The supplier's insolvency is irrelevant because the deposit was locked before the supplier ever saw the order.

#### Expired (user no-show)

| Step | What happens |
|------|-------------|
| 1 | Order transitions to `Expired`; held product released back to inventory |
| 2 | Guardian federation threshold-signs escrow release — root debits escrow, credits supplier's user contract |

### What Freenet contracts enforce

| | Freenet contracts | Guardian federation (FROST) |
|---|---|---|
| **Valid state transitions** | Yes — `verify_delta()` rejects illegal status changes | N/A — guardians act on contract state |
| **Correct signatures** | Yes — only supplier/user can update their fields | Yes — threshold quorum required for root operations |
| **Actual fund custody** | Yes — ledger balances in user contracts | Yes — root escrow controlled by threshold key |
| **Insolvency protection** | Yes — deposits locked in root's escrow upfront | Yes — no single guardian can access funds |
| **Communication channel** | Yes — storefronts for orders, user contracts for direct delivery | No — guardians only sign operations |
| **Works offline/degraded** | Yes — Freenet state persists on hosting nodes | Requires guardian quorum to be reachable |

---

## What is CREAM‑lite and how does the Fedimint escrow module work?

CREAM has two deployment tiers:

- **CREAM‑full**: Custom FROST guardian federation, CURD e‑cash, Lightning gateway — fully self‑contained. This is the primary architecture described throughout this document.
- **CREAM‑lite**: Freenet marketplace contracts + a custom **Fedimint escrow module**. No CREAM‑specific guardian infrastructure — the community rides an existing Fedimint federation for custody, e‑cash, and Lightning.

### Why CREAM‑lite?

Fedimint federations are already deployed in the wild. Communities running them have threshold custody, e‑cash issuance, and Lightning settlement out of the box. What they *don't* have is a decentralised marketplace. CREAM‑lite fills that gap: Freenet provides the censorship‑resistant product listings, orders, and coordination; Fedimint provides the money layer.

This also lowers the barrier to entry. Standing up a 3‑guardian FROST federation with DKG, share resharing, and Lightning is non‑trivial. CREAM‑lite lets a community start with "just add Freenet nodes" on top of infrastructure they already operate.

### The Fedimint escrow module

Fedimint supports custom modules compiled into the `fedimintd` binary. The escrow module adds three operations that map directly to CREAM's order lifecycle:

| Operation | Fedimint concept | What happens |
|-----------|-----------------|--------------|
| **Reserve** | `EscrowInput` (process_input) | Customer's e‑cash is consumed; federation creates an `EscrowRecord` with status `Reserved`, holding the funds |
| **Fulfil** | `EscrowOutput` (process_output) | Order completed — federation releases escrowed amount as new e‑cash to the supplier |
| **Cancel** | `EscrowOutput` (process_output) | Order cancelled — federation refunds escrowed amount as new e‑cash to the customer |

The module lives in three crates following Fedimint's standard pattern:

```
fedimint-escrow-common/   — shared types (EscrowInput, EscrowOutput, EscrowStatus, errors)
fedimint-escrow-server/   — server module (process_input, process_output, audit)
fedimint-escrow-client/   — client module (reserve, fulfil, cancel, list_escrows)
```

### Key design decisions

- **Zero fees**: The escrow module charges no additional fees. The federation's existing fee structure applies to the underlying transactions.
- **Audit**: Reserved escrows are negative liabilities (the federation owes these funds). Fulfilled and cancelled escrows are zero (settled). This integrates with Fedimint's built‑in audit system.
- **No consensus items**: Escrow operations are single‑step — they don't require multi‑round consensus beyond what Fedimint's HBBFT already provides for transaction processing.
- **Keyed by order ID**: Each escrow record is identified by a string order ID that the CREAM marketplace assigns. The escrow module doesn't need to understand order semantics — it just holds and releases funds.

### How CREAM‑lite talks to Fedimint

The CREAM UI (Dioxus/WASM) calls the Fedimint client library to:

1. Create an `EscrowInput` transaction when a customer places an order (locks funds).
2. Create an `EscrowOutput` transaction when the supplier marks the order fulfilled (pays supplier) or the order is cancelled (refunds customer).

The marketplace contracts on Freenet track order state (products, status progression, supplier/customer identity). The Fedimint federation tracks the money (who has how much e‑cash, what's locked in escrow). Neither system needs to trust the other — they share the order ID as a join key, and the UI orchestrates the two‑sided state transitions.

### StartOS deployment

A community running Fedimint on StartOS would install a modified `fedimintd` package that includes the escrow module. Stock Fedimint packages from Start9 won't have the escrow module — it requires either:

1. **Upstream acceptance**: PR the escrow module to the Fedimint project. If accepted, all Fedimint deployments get escrow support.
2. **Custom package**: Build and distribute a `fedimintd` with the escrow module compiled in, packaged as a custom `.s9pk`.

Option 1 is the preferred path. The escrow module is general‑purpose (not CREAM‑specific) and could benefit any Fedimint‑based marketplace.

### Comparison: CREAM‑full vs CREAM‑lite

| Aspect | CREAM‑full | CREAM‑lite |
|--------|-----------|------------|
| **E‑cash** | CURDs (FROST threshold, custom) | Fedimint e‑cash (HBBFT, established) |
| **Custody** | 3+ CREAM guardians | Existing Fedimint federation |
| **Escrow** | Root user contract + FROST signing | Fedimint escrow module |
| **Lightning** | Guardian‑hosted LND | Fedimint's built‑in Lightning gateway |
| **Setup complexity** | Higher (DKG, guardian nodes, Lightning) | Lower (Freenet nodes + existing Fedimint) |
| **Independence** | Fully self‑contained | Depends on external Fedimint federation |
| **Marketplace** | Freenet contracts | Freenet contracts (identical) |

The marketplace layer (Freenet contracts, UI, product listings, order management) is identical in both tiers. Only the money layer differs.

---

## How does cream‑node work? (Postgres backend)

cream‑node (`tools/cream-node/`) is a lightweight axum WebSocket server that implements the Freenet node protocol over Postgres instead of a distributed hash table. It exists for fast deterministic testing and for CREAM‑lite single‑supplier production deployments.

### Wire protocol compatibility

cream‑node listens on the same URL path (`/v1/contract/command?encodingProtocol=native`) and speaks the same bincode‑over‑WebSocket protocol as a Freenet node. Clients send `bincode::serialize(&ClientRequest)` and receive `bincode::serialize(&Ok::<HostResponse, ClientError>(response))`. The UI, test harness, and invariant checker all connect identically — no code changes needed.

### Contract execution

Instead of loading WASM blobs into a sandbox, cream‑node calls the native Rust merge/validate methods from `cream-common` directly:

| Contract | Validation | Merge |
|----------|-----------|-------|
| Directory | `validate_all_signatures()` | LWW by `updated_at` |
| Storefront | `validate(owner)` — products signed, orders signed + deposit | LWW products, monotonic orders |
| User Contract | `validate_update()` — conditional (credits‑only bypass) | Append‑only ledger union, LWW metadata |
| Inbox | `validate_update()` — append‑only, no signature | Union by message ID |
| Market Directory | `validate_all_signatures()` | LWW by `updated_at` |

Contract type is classified at PUT time by attempting to deserialize the parameters as `StorefrontParameters`, `UserContractParameters`, or `InboxParameters`. Empty params default to Directory (with a heuristic to distinguish Market Directory by the presence of `venue_address` in state entries).

### Postgres schema

Two tables:

- **`contracts`** — current state of each contract: instance ID, type, parameters, raw state bytes, JSONB state, WASM code (for key derivation), code hash.
- **`audit_log`** — append‑only log of every PUT and UPDATE, with old state, new state, and update data as JSONB. Protected by Postgres RULES that block UPDATE and DELETE.

Both are written atomically within a single transaction on every state mutation.

### Subscription delivery

Subscriptions use in‑process `tokio::broadcast` channels per contract key. When an UPDATE succeeds, cream‑node serializes an `UpdateNotification` and broadcasts it to all subscribers immediately (sub‑millisecond latency, vs Freenet's 1–5 second propagation delay).

### Differences from Freenet

| Aspect | Freenet | cream‑node |
|--------|---------|------------|
| Storage | Distributed hash table | Postgres |
| Contract execution | WASM sandbox | Native Rust |
| Subscription delivery | Network propagation (1–5s) | In‑process broadcast (< 1ms) |
| Dev cluster | 7 nodes | Single process |
| Audit trail | None | Immutable append‑only |
| Time travel | Not possible | Reconstruct any past state |
| Key derivation | hash(WASM + params) | Identical |
| Wire protocol | bincode over WebSocket | Identical |

---

## Time travel — temporal audit trail

Adapted from the IronClaw sibling project's temporal database pattern.

### Core principle

The `audit_log` table is **append‑only**: rows are only ever INSERTed, never UPDATEd or DELETEd. Postgres RULES enforce this at the database level — even a superuser running `UPDATE audit_log` will silently do nothing.

### What is recorded

Every contract state mutation records:

| Field | Description |
|-------|-------------|
| `entity_type` | Contract type: directory, storefront, user_contract, inbox, market_directory |
| `entity_id` | Contract instance ID (base58) |
| `action` | `put` (initial creation) or `update` (merge) |
| `old_state` | JSONB snapshot before the change (NULL for puts) |
| `new_state` | JSONB snapshot after the change |
| `update_data` | The raw update payload (JSONB, NULL for puts) |
| `ts` | Transaction timestamp (when recorded) |

### Transaction time only

cream‑node records **transaction time** (when the database saw the change), not **valid time** (when the change was "true" in the real world). This is simpler and sufficient for debugging and dispute resolution. Bi‑temporal support (tracking both) is a potential future enhancement.

### Point‑in‑time reconstruction

To reconstruct any contract's state at a past point in time:

1. Query `audit_log` for all entries matching the contract's `entity_id`, ordered by `ts ASC`, up to the desired timestamp.
2. The `new_state` of the last entry is the contract's state at that point.

This is a simple query — no event sourcing replay needed, because we store full snapshots (not just deltas).

### Phased approach

- **Phase 1** (current): Audit log with full state snapshots on every mutation.
- **Phase 2** (planned): `/admin/audit` HTTP endpoint for querying history and point‑in‑time reconstruction.
- **Phase 3** (future): Native Postgres temporal tables (`PERIOD FOR` / `SYSTEM_TIME`) for richer temporal queries, when Postgres adds full SQL:2011 temporal support.

---

## Hybrid architecture: Postgres for ledgers, Freenet for public data? (Future idea)

A production deployment could split contract storage across two backends, using each where it is strongest.

### The problem with all‑Freenet

Multi‑step operations on Freenet are not atomic. Placing an order requires three separate UPDATEs — storefront (add order), customer user contract (escrow debit), root user contract (escrow credit). Any of those UPDATEs can timeout, fail to propagate, or arrive out of order. This is the root cause of the CURD conservation invariant violations we've seen in testing: a debit is recorded but the corresponding credit is lost, and 50 CURD vanishes.

Freenet has no concept of a transaction spanning multiple contracts.

### The problem with all‑Postgres

cream‑node solves the atomicity problem — wrap all three UPDATEs in a single Postgres transaction, roll back on any failure. But a single Postgres instance is a single point of failure, a single point of censorship, and a single point of data custody. Every user's balance and ledger sits on one server.

### The hybrid split

| Contract type | Backend | Rationale |
|---|---|---|
| **User contracts** (balances, ledgers) | **Postgres** | ACID transactions prevent lost updates. Audit trail for dispute resolution. Instant balance queries. Multi‑contract operations (escrow debit + credit) can be atomic. |
| **Directory** (supplier listings) | **Freenet** | Public, read‑heavy, low write contention. LWW merge handles concurrent updates naturally. Censorship‑resistant distribution. |
| **Storefronts** (products, orders) | **Freenet** | Supplier's public face — censorship resistance matters. Each storefront is mostly single‑writer (the supplier). Orders flow through the ledger for settlement. |
| **Inbox** (messages) | **Freenet** | Privacy benefits from distributed storage. Messages are append‑only, naturally convergent. |
| **Market directory** | **Freenet** | Public registry, same rationale as directory. |

The key insight: **user contracts are bank ledgers** — you want ACID guarantees. **Storefronts and directories are bulletin boards** — you want censorship resistance and availability.

### How it would work

1. The UI connects to both a Freenet node (for directory/storefront/inbox) and a cream‑node (for user contracts).
2. Placing an order:
   - UPDATE storefront on Freenet (add order entry) — can be retried idempotently.
   - UPDATE customer + root user contracts on cream‑node in a single Postgres transaction (escrow debit + credit) — atomic, all‑or‑nothing.
3. If the Freenet UPDATE fails, the Postgres transaction hasn't happened, so no CURD is moved. Retry safely.
4. If the Postgres transaction fails, the order entry on the storefront is harmless (no funds moved). The order can be retried or will expire.

### Transaction isolation for multi‑step operations

Even without the full hybrid split, cream‑node already enables transactional grouping of related contract updates. The `get_and_update_contract` method uses `SELECT FOR UPDATE` to prevent lost updates on a single contract. Extending this to multi‑contract transactions is straightforward:

```
BEGIN;
  SELECT ... FROM contracts WHERE instance_id = $customer_id FOR UPDATE;
  SELECT ... FROM contracts WHERE instance_id = $root_id FOR UPDATE;
  -- merge escrow debit into customer state
  -- merge escrow credit into root state
  UPDATE contracts SET state_bytes = ... WHERE instance_id = $customer_id;
  UPDATE contracts SET state_bytes = ... WHERE instance_id = $root_id;
  INSERT INTO audit_log ...;
  INSERT INTO audit_log ...;
COMMIT;
```

If either merge fails validation, the entire transaction rolls back. No partial state. No lost CURD.

### Why user contracts on Freenet is the wrong split

It might seem natural to put the privacy‑critical data (user contracts / balances) on Freenet for distributed storage. But user contracts are where the *most* write contention lives — escrow debits/credits from multiple orders, CURD allocations, toll payments, checkpoint updates. Putting the most contention‑prone data on the least reliable backend (no transactions, eventual consistency, propagation timeouts) maximises corruption risk.

Privacy for user contracts is better achieved through encryption at rest and threshold access control (FROST guardians), not through distributing unencrypted state across a DHT.

### Deployment scenarios

- **CREAM‑lite (single supplier)**: All contracts on cream‑node. Simplest. One process, one database, full ACID.
- **Community deployment**: User contracts on cream‑node (run by guardians), public contracts on Freenet. Best of both.
- **Full decentralisation**: All contracts on Freenet. Maximum censorship resistance, but accept the atomicity trade‑offs.

The hybrid model is not an all‑or‑nothing choice — communities can start with cream‑node for everything and selectively migrate public contracts to Freenet as the network matures.

---

## Cross‑backend atomicity: read‑after‑write, sagas, and why you need both

The hybrid architecture (Postgres for ledgers, Freenet for public contracts) introduces a fundamental distributed systems problem: how do you make a multi‑step operation atomic when the steps span two different backends with different consistency guarantees?

### The order placement problem

Placing an order requires three writes that must all succeed or all be reversed:

1. **Storefront** (Freenet): add the order entry
2. **Customer user contract** (Postgres): escrow debit — lock the deposit
3. **Root user contract** (Postgres): escrow credit — receive the locked funds

If step 1 succeeds but step 2 fails, there's an order on the storefront with no funds backing it. If steps 1–2 succeed but step 3 fails, 50 CURD has been debited from the customer but never credited to escrow — the CURD conservation invariant is violated and 50 CURD vanishes.

This is the exact failure mode we've observed in testing with Freenet: UPDATE timeouts mid‑sequence leave the system in a half‑written state.

### Read‑after‑write: the confirmation gate

**Read‑after‑write** answers a narrow question: *did this single write actually land?*

After sending an UPDATE to Freenet, you GET the contract and verify your write is reflected in the returned state. If it isn't, you wait and retry. The retry‑with‑backoff logic in the current codebase is a rough form of this — keep GETting until the state matches expectations.

Read‑after‑write is necessary because Freenet's UPDATE response only confirms the local node accepted the write, not that it has propagated or will be visible on a subsequent GET (the "stale GET after UpdateResponse" issue, freenet‑core#3357).

Without read‑after‑write, you cannot reliably sequence dependent operations. You might proceed to step 2 believing step 1 succeeded, when it actually didn't.

### Saga pattern: the compensation plan

**A saga** answers a broader question: *what do I do when step N fails after steps 1 through N−1 already succeeded?*

Each step in a saga has a corresponding **compensating action** — a reverse operation that undoes its effect. When a step fails, the saga walks backwards through the completed steps, executing their compensations.

For the order flow with the hybrid architecture:

```
Step 1: UPDATE storefront (Freenet) — add order as PendingEscrow
  ↓ read‑after‑write confirms order visible
Step 2: BEGIN Postgres transaction
          UPDATE customer contract — escrow debit
          UPDATE root contract     — escrow credit
        COMMIT
  ↓ transaction committed
Step 3: UPDATE storefront (Freenet) — transition PendingEscrow → Reserved
  ↓ read‑after‑write confirms Reserved status
Done.
```

Compensation table:

| Failure point | What happened | Compensation |
|---|---|---|
| Step 1 fails | Nothing written | None needed — retry or abort |
| Step 2 fails | Order is PendingEscrow on storefront, no funds moved | UPDATE storefront to cancel the PendingEscrow order |
| Step 3 fails | Funds locked in Postgres, order stuck at PendingEscrow | Retry step 3 (idempotent); or after timeout, reverse the Postgres transaction and cancel the order |

### Why you need both, not either‑or

Read‑after‑write and sagas are complementary — they solve different layers of the same problem.

- **Read‑after‑write without saga**: You can confirm each step landed, but when one fails permanently you have no plan. The system is stuck in an inconsistent state with no automated recovery.
- **Saga without read‑after‑write**: You have compensation logic, but you can't reliably tell *when* to trigger it. Is the Freenet write still propagating (wait longer) or did it genuinely fail (compensate now)? Without confirmation gates, you'll either compensate too early (undoing a write that would have succeeded) or too late (leaving the system inconsistent while you wait).

The retry‑with‑backoff logic we have today is the read‑after‑write half. What's missing is the saga half — the compensation logic that kicks in when a step fails after previous steps succeeded. Currently, when step 2 times out, we just… lose 50 CURD.

### Why the hybrid architecture simplifies this dramatically

In the all‑Freenet world, every step requires read‑after‑write confirmation and every step needs its own compensation. That's six read‑after‑write cycles and three compensation paths, all against an unreliable backend.

The hybrid split collapses the hard part. Steps 2 and 3 (escrow debit + credit) become a single Postgres transaction — no read‑after‑write needed (ACID guarantees it), no compensation needed (it either commits or rolls back atomically). The saga reduces to:

1. Write PendingEscrow to Freenet (read‑after‑write to confirm)
2. Atomic Postgres transaction for funds
3. Write Reserved to Freenet (read‑after‑write to confirm)

Only the Freenet steps need the distributed systems machinery. And those steps are *idempotent* storefront updates where compensation is trivial (cancel the order).

### Background reconciler

Even with read‑after‑write and compensation logic, edge cases remain: the process crashes between step 2 and step 3, or a compensation itself fails. A background reconciler provides the safety net:

- Periodically scan for `PendingEscrow` orders older than a threshold (e.g. 60 seconds).
- Check Postgres: do matching escrow entries exist?
  - **Yes**: the saga stalled between steps 2 and 3 — retry the Reserved transition.
  - **No**: step 2 failed or was never attempted — cancel the order on the storefront.
- The reconciler is idempotent and convergent. Run it as often as you like.

This is the standard pattern in event‑driven architectures: optimistic fast path (saga) plus pessimistic slow path (reconciler). The saga handles 99% of cases instantly. The reconciler catches the 1% that fall through the cracks.

---

## Could multiple cream‑node instances sync via gossip? (Future idea)

Yes. The CRDT properties of CREAM's contract types make multi‑node Postgres replication viable with high confidence. This would be a "Freenet‑lite" — decentralized replication without the full Freenet DHT.

This design is a shared concern with the sibling project **FreeClawdia** (IronClaw), which faces the same problem: multiple autonomous Postgres instances (Gary, Emma, Iris) that need to sync without a central coordinator. Both projects use append‑only audit logs, temporal reconstruction, and LWW merge semantics. The gossip protocol design should work for both — the contract‑level merge layer is CREAM‑specific, but the audit log replication layer is project‑agnostic.

### Why it works

1. **Deterministic merge** — all 5 contract types use CRDTs: LWW by timestamp (directory, storefront products, market directory, user contract metadata), monotonic status ordinals (orders), set‑union (ledger entries, inbox messages). Given the same set of updates, any two nodes converge to identical state regardless of delivery order.
2. **Idempotent merges** — applying the same update twice is a no‑op. Gossip can safely re‑deliver without corruption.
3. **Built‑in anti‑entropy** — the existing `summarize()` / `delta()` methods on each contract type are purpose‑built for efficient diffing. A periodic "give me what I'm missing" exchange uses these directly.
4. **Append‑only audit log** — only grows, never mutates. Sync is just "send entries after my latest `ts`".

### The audit log as a replication stream

The `audit_log` table is append‑only with timestamps and UUIDs (or serial IDs) — exactly the properties needed for conflict‑free replication. This insight emerged from both CREAM and FreeClawdia arriving at the same temporal database design independently:

- **CREAM's audit_log**: records every contract state change (PUT, UPDATE) with `old_state`, `new_state`, `entity_type`, `entity_id`, and immutability enforced by Postgres RULES.
- **FreeClawdia's audit_log**: records every mutation (settings, conversations, extensions, skills, routines) with JSON diffs, also append‑only with timestamp ordering.

Both are structurally identical: append‑only, timestamped, entity‑keyed, JSON‑valued. A gossip layer that works for one works for both.

**Replication mechanics:**

1. Each instance periodically asks peers: "what's your latest audit_log timestamp?"
2. Send all entries newer than the peer's watermark.
3. Receiving instance inserts entries idempotently (`INSERT ... ON CONFLICT DO NOTHING` on the entry ID).
4. For CREAM: after ingesting audit entries, replay the contract merges they describe to update materialized state. The `new_state` in each entry provides the post‑merge snapshot, but re‑merging from the `update_data` field provides verification.
5. For FreeClawdia: after ingesting audit entries, the temporal reconstruction layer (`audit_as_at()`) works identically whether entries originated locally or arrived via gossip.

The audit log is a **CRDT by construction** — append‑only with unique IDs means any set‑union of entries from multiple nodes converges to the same log (modulo ordering, which is resolved by timestamp).

### Gossip protocol sketch

- Each node broadcasts raw UPDATE payloads (not merged state) to peers.
- Receiving node applies `merge()` locally via the same native Rust logic.
- Periodic anti‑entropy: exchange `summarize()` digests, compute `delta()`, send missing data.
- Audit log replication via watermark polling.

### Two layers of replication

The gossip protocol operates at two distinct layers, and it's important to understand what each provides:

**Layer 1: Audit log sync (append‑only, conflict‑free)**

This is the easy part. Audit entries have unique IDs and timestamps. Replication is pure set‑union — no conflicts possible. Every entry that exists on any node eventually exists on all nodes. This gives you a complete, globally consistent history of what happened and when, which is sufficient for temporal queries ("what was this contract's state at 3pm Tuesday?").

**Layer 2: Materialized state sync (merge‑dependent, convergent)**

This is the harder part. Each cream‑node maintains a `contracts` table with the current materialized state. When an audit entry arrives from a peer describing an UPDATE, the receiving node must apply the same merge logic to its local state. Because the merges are CRDTs, the result converges regardless of application order — but the intermediate states may differ between nodes until all entries have propagated.

FreeClawdia only needs Layer 1 — its entities (settings, conversations, extensions) are simple key‑value pairs where LWW on the audit entries is sufficient. CREAM needs both layers because contract state is the product of accumulated merges, not just the latest write.

### Transport and discovery

**Open design questions** (shared with FreeClawdia):

| Question | Options | Notes |
|---|---|---|
| **Transport** | HTTP polling, WebSocket push, Postgres logical replication | HTTP polling is simplest; WebSocket gives real‑time; logical replication is Postgres‑native but tightly coupled |
| **Discovery** | Static config (`--peers`), mDNS for LAN, rendezvous service | CREAM already has a rendezvous service; static config is simplest for small deployments |
| **Selective sync** | All audit entries, or filtered by entity type? | FreeClawdia may want to share settings but not conversation content; CREAM may want to share directory but not user contracts |
| **Privacy** | Some instances may not want to share all data with all peers | Encryption at rest, per‑entity access control, or separate gossip channels per data classification |
| **Topology** | Full mesh, star (one hub), or random fanout | Full mesh is fine for 3–7 nodes; random fanout needed at scale |

For both projects, the initial implementation would likely be **HTTP polling with static peer configuration** — the guardian nodes already use this pattern for FROST DKG and signing rounds. A peer list via `--peers` CLI arg, periodic polling via `tokio::interval`, and idempotent INSERT on the receiving end.

### Known risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Clock skew** | LWW picks wrong winner | NTP discipline, or hybrid logical clocks (HLC) |
| **Partition healing** | Conflicting metadata — LWW resolves but loser silently disappears | Same as Freenet behavior; acceptable for CREAM's use case |
| **Audit log divergence** | Nodes applying updates in different order have different `old_state` snapshots | Each node's log is locally correct; not globally identical but acceptable |
| **Append‑only ledger ordering** | `UserContractState.ledger` entries arrive out of order | Merge deduplicates by `(tx_ref, kind)` and re‑sorts by timestamp — converges correctly |
| **Unbounded growth** | Append‑only means the audit log grows forever | Retention policies: compact entries older than N days, keep only latest snapshot per entity |
| **Entity‑level conflicts** | Two nodes rename the same thread (FreeClawdia) or update the same product (CREAM) concurrently | LWW by timestamp — one wins, one loses silently. Acceptable for both projects' use cases |

### Relationship to existing sync approaches

| Approach | Topology | Conflict resolution | Offline support | Complexity |
|---|---|---|---|---|
| **Freenet DHT** | Fully decentralized P2P mesh | Contract‑level merge (CRDT) | No (requires network) | High |
| **Turso cloud replica** (FreeClawdia current) | Star (Turso cloud hub) | LWW at Turso layer | Yes (local replica) | Low |
| **Gossip over audit log** (this proposal) | Peer‑to‑peer mesh | CRDT merge + LWW | Yes (fully autonomous) | Medium |
| **Postgres logical replication** | Primary → replica | Write‑ahead log replay | No (streaming required) | Medium |

The gossip approach occupies the sweet spot: fully decentralized like Freenet but without the DHT complexity, and peer‑to‑peer like logical replication but without requiring a designated primary. Each instance is a full read‑write peer.

### Status

**Design stage** — architecture documented, not yet implemented. The merge semantics are already proven (they're the same ones Freenet uses). The audit log infrastructure exists in both CREAM and FreeClawdia. The engineering work is the gossip layer itself: peer discovery, reliable delivery, anti‑entropy protocol, and connection management.

**Shared design** — this is actively coordinated between CREAM (Henry) and FreeClawdia (Clawdia) via their respective IAQ files. Implementation in either project would validate the design for both.

---

## Testing strategy: cream‑node as control environment

cream‑node's most immediate and valuable role is as a **control environment** for isolating bugs. Before cream‑node, every test failure was ambiguous — is it our contract logic? A Freenet subscription timeout? A stale GET? A propagation delay? Diagnosing a single failure could consume hours of log trawling across 7 nodes. With cream‑node, the diagnostic flow is binary.

### The two‑backend diagnostic

| Fails against cream‑node? | Fails against Freenet? | Diagnosis |
|---|---|---|
| Yes | — | **Our bug.** Fix it. Don't touch Freenet. |
| No | Yes | **Freenet bug** (or a distributed systems edge case). Investigate with confidence that our logic is sound. |
| No | No | Working correctly. |
| Yes | No | Shouldn't happen (cream‑node is stricter), but would indicate a cream‑node implementation gap. |

### Why this works

cream‑node eliminates every source of non‑determinism that Freenet introduces:

- **No network propagation** — writes are visible immediately via Postgres ACID.
- **No DHT routing** — no "could not find peer" failures, no routing through a gateway bottleneck.
- **No subscription broadcast timing** — in‑process `tokio::broadcast`, sub‑millisecond delivery.
- **No stale GET after UpdateResponse** — `SELECT FOR UPDATE` guarantees read‑your‑writes.
- **No merge race conditions** — serialized Postgres transactions, not concurrent distributed merges.
- **Full audit trail** — every state change recorded immutably, reconstructible at any point in time.

If a test passes against cream‑node, the contract logic, merge semantics, UI behavior, CURD conservation invariants, and subscription flows are all verified correct. Any failure against Freenet is therefore attributable to the network layer.

### Freenet upgrade workflow

When a new Freenet release arrives:

1. **Baseline**: `cargo make test-node-pg` — confirm all 12 integration steps + 6 stress tests pass against cream‑node. Our code is clean.
2. **Upgrade**: bump `freenet` and `freenet-stdlib` versions.
3. **Test**: `cargo make test-node` — run the identical tests against a live 7‑node Freenet cluster.
4. **Diagnose**: any new failures are almost certainly Freenet regressions. We already have:
   - The exact test case that reproduces the bug.
   - The expected behavior (from the cream‑node run).
   - The audit trail showing what *should* have happened.
   - A concrete, minimal reproduction to attach to a PR.

This is the workflow that produced our Freenet contributions: the piped stream overflow bug (PR #3455, eliminated ~40% PUT failure rate), the stale GET issue (#3357), and the gateway topology problem (#3362). Each was found because we were exercising the system hard enough to hit edge cases, and could definitively rule out our own code as the cause.

### Performance comparison

| Metric | Freenet (7 nodes) | cream‑node |
|---|---|---|
| Integration tests (12 steps) | 2–5 minutes | **1.6 seconds** |
| Stress tests (6 tests) | 30–60 seconds | **5 seconds** |
| Full fixture startup | 60–90 seconds | **33 seconds** |
| E2E tests (47 tests) | 8–15 minutes | **4 minutes** |
| Subscription notification latency | 1–5 seconds | **< 1 millisecond** |
| Flaky test rate | ~10–15% | **0%** |

The speed difference means developers can run the full test suite on every change, not just before commits. The zero flakiness means a red test always means a real bug, never "just retry it."

### Cumulative fixture testing

Both backends support CREAM's cumulative fixture pattern: integration tests populate the database with realistic data (3 suppliers, 6 customers, products, orders, CURD allocations, market directory entries), and E2E tests run on top of that state. cream‑node's `ON CONFLICT` PUT semantics and merge‑on‑PUT behavior ensure the E2E fixture builds correctly on the integration test state, exactly as Freenet's merge semantics would.

The CURD conservation invariant checker (`check-invariants`) works identically against both backends — it connects via the same WebSocket protocol and verifies the same `total == 1,000,000 CURD` invariant. A passing invariant check against cream‑node is a guarantee that the contract logic conserves funds correctly; a failing check against Freenet with the same test sequence is proof of a network‑level issue.

---

## How do guardian nodes run in production? (StartOS)

CREAM guardian nodes are packaged as a service for **StartOS** (from [Start9 Labs](https://start9.com)) — a self-hosting Linux OS where each service runs in an isolated Docker container. StartOS provides a web UI for service management (no CLI needed), automatic Tor hidden services for every installed service, a dependency system that auto-wires services together, health checks, backups, and updates. StartOS itself is written primarily in Rust.

Target hardware: 8-16 GB RAM, quad-core CPU, 1-2 TB NVMe. The Start9 Server One (2025 model: Ryzen 7-5825U, 16 GB DDR4, 2 TB NVMe) is the reference platform.

### Why StartOS for CREAM guardians?

1. **Bitcoin + Lightning already packaged** — `bitcoind`, `lnd`, `c-lightning` are in the StartOS marketplace. Guardian nodes declare them as dependencies and StartOS auto-configures the connections.
2. **Multi-box federation** — each Start9 box runs one guardian. Three boxes = a 2-of-3 federation for DKG, resharing, and signing rounds.
3. **Realistic deployment model** — suppliers in production would run exactly this: a Start9 box with Bitcoin + Lightning + CREAM.
4. **Tor networking built in** — services get `.onion` addresses automatically, enabling guardian communication over real network conditions without manual port forwarding.
5. **Resource constraint as design target** — if CREAM runs well on a Start9 box alongside Bitcoin + Lightning, it'll run well anywhere.

### Package architecture

CREAM is bundled as a single StartOS service: Freenet node + CREAM contracts + UI in one Docker container. No separate Freenet package — Freenet is purpose-built for CREAM at this stage.

```
cream-startos/
  manifest.yaml         # Service metadata, deps, ports, health checks
  Dockerfile            # Builds Freenet + CREAM into one image
  docker_entrypoint.sh  # Starts Freenet node, serves CREAM UI
  scripts/embassy.ts    # Config UI, dependency wiring, health checks
  instructions.md       # User-facing docs in StartOS UI
  icon.png
  LICENSE
  Makefile
```

### Dependencies declared in manifest

| Dependency | Purpose | Critical? |
|-----------|---------|-----------|
| `bitcoind` | Bitcoin full node for blockchain validation | Yes |
| `lnd` or `c-lightning` | Lightning channels for CURD peg-in/peg-out | Yes |

### Interfaces

| Port | Protocol | Purpose |
|------|----------|---------|
| 3001 | WebSocket | Freenet node API (for mobile customer connections) |
| 8080 | HTTP | CREAM UI (supplier dashboard) |
| 8787 | HTTP | Rendezvous service (optional, one guardian hosts this) |

### Inter-service communication

StartOS auto-configures Tor-based service discovery. The CREAM service connects to Bitcoin Core via RPC at `<bitcoind-onion>.onion:8332` and to Lightning via gRPC at `<lnd-onion>.onion:10009`, with credentials auto-injected via TypeScript dependency procedures.

### Guardian ceremony testing plan

**Phase 1: Single box**
- Package CREAM as a StartOS service
- Install on one Start9 box alongside `bitcoind` + `lnd`
- Verify Freenet node starts, UI serves, Lightning connectivity works
- This is the 1-of-1 genesis guardian (degenerate FROST case)

**Phase 2: Three boxes**
- Install CREAM on all three Start9 boxes
- Run DKG ceremony: three guardians establish a 2-of-3 threshold key
- Root public key established at genesis, shared across all boxes
- Verify guardians can discover each other (via rendezvous or direct Tor addresses)

**Phase 3: Operational testing**
- Test threshold signing: escrow release requires 2-of-3 guardian cooperation
- Test guardian failover: take one box offline, verify 2-of-3 quorum still signs
- Test key resharing: add a 4th guardian (3-of-5), verify root public key unchanged
- Test CURD peg-in/peg-out via Lightning across the guardian federation

### Resource budget

Assuming a Start9 Server One (16 GB RAM, quad-core):

| Service | Estimated RAM | Notes |
|---------|-------------|-------|
| Bitcoin Core | 2-4 GB | Pruned mode possible for testing |
| LND | 500 MB - 1 GB | |
| Freenet node | 500 MB - 1 GB | TBD — needs profiling |
| CREAM UI/contracts | 200-500 MB | Dioxus WASM served via Freenet |
| OS + StartOS | 1-2 GB | |
| **Total** | **~5-8 GB** | Comfortable on 16 GB box |

CREAM (Freenet + contracts + UI) should target **< 1.5 GB RAM** to leave headroom.

### Packaging steps

1. Get Start9 boxes running with `bitcoind` + `lnd`
2. Install `start-sdk` from the [start-os repo](https://github.com/Start9Labs/start-os)
3. Create `cream-startos` wrapper repo from [hello-world template](https://github.com/Start9Labs/hello-world-startos)
4. Write Dockerfile that builds Freenet + CREAM from source
5. Write `manifest.yaml` with dependencies on `bitcoind` + `lnd`
6. Write TypeScript config procedures for dependency wiring
7. Build `.s9pk` package: `start-sdk pack`
8. Sideload onto Start9 box: `start-cli package install cream.s9pk`
9. Test single-box guardian, then expand to three boxes

### Key resources

| Resource | URL |
|----------|-----|
| StartOS docs | https://docs.start9.com/ |
| Packaging guide | https://docs.start9.com/0.3.5.x/developer-docs/packaging |
| Manifest spec | https://docs.start9.com/0.3.5.x/developer-docs/specification/manifest |
| Dependencies spec | https://docs.start9.com/0.3.5.x/developer-docs/specification/dependencies |
| JS procedures | https://docs.start9.com/0.3.5.x/developer-docs/specification/js-procedure |
| Hello world template | https://github.com/Start9Labs/hello-world-startos |
| Bitcoin Core wrapper | https://github.com/Start9Labs/bitcoind-startos |
| LND wrapper | https://github.com/Start9Labs/lnd-startos |
| StartOS GitHub | https://github.com/Start9Labs/start-os |
| Start9 marketplace | https://marketplace.start9.com/ |

---

## Is CREAM one big network or many small ones?

**Many small ones.** CREAM is open-source co-operative software, not a national franchise. Each community runs their own independent instance.

### Why co-ops, not a single network?

Raw dairy is inherently local. A customer in Perth has no use for a directory of NSW farms. The value proposition is **knowing your farmer** — visiting the farm, seeing the animals, building trust through repeated in-person transactions. A single national directory would be mostly noise.

Guardian federations also work best when guardians know and trust each other. Three farmers in the Hunter Valley running guardian nodes on their StartOS devices makes sense. Three strangers spread across a continent doesn't.

Each co-op is a completely independent CREAM instance:

- Its own Freenet network and guardian federation
- Its own directory of suppliers and customers
- Its own e-cash token supply (see below)
- Its own branding, name, and community governance

The software is identical, but the communities are sovereign. "Hunter Valley Raw" and "Gippsland Fresh" might both run CREAM under the hood, with their own identities and cultures.

### Can each co-op name their own e-cash token?

Yes. "CURD" is just CREAM's default token name. Each co-op can brand their e-cash however they like — CURD, MOO, BLEAT, whatever resonates with their community. The underlying mechanics (FROST threshold signing, Lightning peg-in/out, escrow, toll rates) are identical regardless of what the token is called.

This reinforces that each co-op is community-owned, not a franchise stamped from a template.

### What about shared guardians?

This is where it gets interesting. The biggest barrier to launching a new co-op is guardian hardware — each guardian needs a StartOS device running a Freenet node, Bitcoin Core, and LND. If three farmers each need to buy a device before they can launch, that's a significant upfront commitment.

**The solution: shared guardian hardware across co-ops.**

A single StartOS device can run multiple guardian daemon instances — one per co-op — each with its own FROST key share, port, and Freenet node connection. The hardware is shared, the cryptographic identities are isolated. A guardian in Bathurst could serve both a Blue Mountains co-op and a Central West co-op simultaneously.

This means:

- **Guardians don't need to be farmers.** A trusted community member, a local tech enthusiast, anyone who believes in the mission can buy one device and participate in multiple federations.
- **Proximity matters** — for latency (FROST signing rounds are HTTP calls between guardians) and for trust (you want guardians who are known to the communities they serve). Geographic neighbors naturally fit both criteria.
- **The barrier drops** from "find three people willing to buy devices" to "find three people in the region who already have one."
- **A mesh of trust emerges** — overlapping guardian participation creates informal connections between neighboring co-ops without any formal federation protocol.

### How does a new supplier bootstrap a co-op?

A supplier buys one StartOS device. This single box runs their Freenet node, guardian daemon, Bitcoin Core, and LND — it's their shop front **and** one-third of the guardian federation.

They then need two more guardians. Rather than anonymous discovery (which would mean trusting strangers with the co-op's money supply), the model is **introduction through the existing network**:

1. The supplier contacts a neighboring co-op's admin — via CREAM itself, or through the local farming community.
2. That admin vouches for two of their existing guardians who have spare capacity and are geographically nearby.
3. The supplier has a conversation with them, establishes trust, then runs the DKG signing ceremony together.

This preserves the human trust chain that makes CREAM work. The neighboring co-op's reputation is on the line when they make the introduction. It's low-friction — you're not cold-calling strangers — but there's social accountability behind every guardian relationship.

The real-world cost to bootstrap: **one StartOS device and an introduction from a neighboring co-op.** As the network grows, finding nearby guardians with spare capacity gets easier, not harder.

### What about delivery?

CREAM supports two collection models, both designed to preserve privacy at every layer.

**Direct pickup (default).** The supplier publishes a postcode area and arranges pickup details via private chat with the customer. No physical address needs to appear on the storefront listing.

**Supplier delivery.** The supplier offers delivery within a local radius. The **customer** provides a delivery address (visible only to the supplier), and the supplier decides whether to service it. This inverts the usual model — the supplier never publishes their own location, making them invisible to online snooping. It's not Amazon-style shipping through a courier; it's a farmer doing a milk run within their local area.

**The milkman model.** This is where it gets powerful. A delivery person operates as a reseller within the co-op:

1. The **milkman** registers as a supplier on the co-op (offering delivery within their area).
2. They negotiate bulk pickup from the **farmer** via private chat — only the milkman knows the farm's location.
3. **End customers** order from the milkman's storefront and provide their delivery address — only the milkman knows where they live.
4. The **farmer** never learns who the end customers are or where they live. The **customers** never learn where the farm is. The **milkman** is the only party who bridges both worlds.

This creates three-party privacy:

| Party | Knows supplier location? | Knows customer addresses? |
|-------|-------------------------|--------------------------|
| Farmer | Yes (it's them) | No |
| Milkman | Yes | Yes |
| Customer | No | Yes (it's them) |

The milkman takes on the visibility risk voluntarily, in exchange for the resale margin. They have a direct trust relationship with both the farmer and the customers, so there's mutual accountability.

No special infrastructure is needed — the milkman is just another supplier in the co-op who happens to source from other suppliers rather than from their own cows. CURD flows naturally: customers pay the milkman, milkman pays the farmer, all on-network. Chat handles the scheduling. The existing order and escrow system handles the money.

**The reseller shopfront.** The delivery person doesn't have to deliver at all. They can operate a physical shop — picking up from the farmer in bulk, then reselling through a standard CREAM storefront with published hours and a shop address. Customers browse and collect in person, just like at a farm gate, but the farmer behind the products remains completely private. The reseller cops the exposure; the farmer doesn't. No new features needed — this is a standard CREAM supplier who happens to source from other suppliers.

These models form a spectrum of distribution, all supported by the same platform:

| Model | Farmer visible? | Customer visits | New features needed |
|-------|----------------|-----------------|---------------------|
| Farm gate | Yes | Farm | None |
| Farmer delivers | No | Home | Minimal (address on order) |
| Milkman delivers | No | Home | None |
| Reseller shopfront | No | Shop | None |
| Farmer's market | Yes (at market) | Market venue | Market features |

What CREAM deliberately does **not** support is anonymous long-distance shipping through courier networks. That would require chain-of-custody tracking, dispute resolution, and reputation systems — all the complexity of a generic marketplace. CREAM's delivery model is personal and local.

### How do farmer's markets work with CREAM?

Physical farmer's markets are a natural partner for CREAM. The market organizer acts as a **venue curator** — they provide the physical location, opening hours, and a curated list of participating suppliers. CREAM handles the product catalogue, reservations, and payments.

**How it works:**

1. The **market organizer** registers a market on CREAM — a special entity with a venue address, schedule (e.g., "Saturdays 7am–1pm"), and a list of participating suppliers.
2. **Farmers** opt in to list some or all of their products at the market. A product can appear on both the farmer's own storefront (for farm gate collection) and the market's combined listing.
3. **Customers** browse the market's aggregated catalogue — seeing products from all participating farmers in one place. They reserve products and pay deposits via CURD, just like any other CREAM order.
4. On market day, the customer **collects at the market venue** from the farmer's stall. The reservation ensures the farmer brings the right quantity.

**What the organizer sees:**

A market dashboard showing all participating suppliers, their committed products for the next market day, and aggregate reservation volumes. This helps with stall planning, logistics, and communication. The organizer can message all suppliers through CREAM's existing chat and inbox.

**What the organizer does NOT do:**

- **Handle money.** CURD flows directly from customer to farmer. The organizer may charge a stall fee as a separate CURD transaction, but that's between the organizer and each farmer — CREAM doesn't enforce it.
- **Hold inventory.** Each farmer's products have one source of truth on their own storefront contract. The market listing is a view over multiple storefronts, not a separate inventory.
- **Guarantee quality.** The organizer curates which suppliers participate, but each farmer maintains their own reputation through direct customer relationships.

**What's needed beyond current CREAM:**

- **Market entity** — distinct from a regular supplier. Has a venue location, schedule, and a roster of participating suppliers.
- **Supplier opt-in** — a farmer links selected products to one or more markets. The product becomes discoverable through the market's combined listing.
- **Market-scoped orders** — when a customer reserves through the market listing, the order is tagged with the market venue as the collection point. Payment still flows directly to the farmer.
- **Organizer dashboard** — aggregate view of participating suppliers, committed products, and reservation volumes for upcoming market days.

**Why this matters:**

Farmer's markets are the existing real-world trust network for local food. Market organizers already curate suppliers, attract customers, and manage logistics. CREAM doesn't replace the market — it enhances it with online reservations, guaranteed availability, and private payments. The organizer becomes a bridge between CREAM's digital co-op and the physical market community, naturally onboarding both suppliers and customers who might never have sought out CREAM independently.

### How does a customer find their local co-op?

Initially, word of mouth — the same way people find their local farmer's market. A co-op's rendezvous service endpoint and postcode coverage area are all that's needed.

In future, a lightweight discovery layer could let co-ops advertise their existence (postcode range + rendezvous URL) so new customers can find the nearest one. But that's a trivial addition, not an architectural commitment.

---

## Contract Permanence and the Soft-Fork Model

### The constraint

A Freenet contract's identity is `BLAKE3(WASM bytecode || Parameters)`. Change one byte of contract code and you get a completely different contract on the network. The old contract continues to exist with its state. There is no update-in-place, no redirect, no migration protocol. This is by design — the contract key is a cryptographic commitment to exact behavior.

This means: **once a CREAM contract is deployed to production, its code is permanent.** A "new version" is a new contract with a new key, and all existing state (directory listings, storefronts, orders, wallets, CURD balances) lives on the old one.

### The analogy: Bitcoin soft forks and safe schema migrations

This is equivalent to a Bitcoin soft fork — old nodes still validate, new nodes understand more. Or in relational database terms: the only safe migration is `ALTER TABLE ADD COLUMN ... DEFAULT ...`. You can enhance, you cannot break.

The contract is a constitution. Its laws will not change, although they may be enhanced to deal with new situations they couldn't previously deal with.

### What this means in practice

**Things we CAN do after launch (additive, backward-compatible):**

- Add new optional fields to state structs (via `#[serde(default)]`) — old contract code deserializes new state safely, ignoring unknown fields
- Add new enum variants that old code falls through to a default case
- Add new interpretation of existing data in the UI (the UI evolves freely — it's just a web app)
- Extend the `extra` / extension fields if we design them in from the start

**Things we CANNOT do after launch (require new WASM = new contract):**

- Fix a bug in `merge()` logic
- Change the conflict resolution strategy
- Add new validation rules to contract code
- Remove or rename existing state fields
- Change the meaning of existing fields
- Add fundamentally new contract-enforced capabilities (e.g. multi-sig orders, new escrow rules)

### Design rules that follow

These rules are **non-negotiable** for any contract code that ships to production:

1. **`#[serde(default)]` on every struct field.** Old contract code must deserialize state written by future UI versions that added fields. This is the mechanism that makes soft-fork evolution possible.

2. **`#[serde(flatten)]` or `extra: HashMap<String, Value>` extension points.** Where we can anticipate future needs, provide generic extension fields that contract code passes through without interpreting.

3. **Merge rules must be as generic as possible.** LWW-by-timestamp is good — it works regardless of what fields exist. Monotonic status ordinals are good — new statuses can be appended. Avoid merge logic that depends on exhaustive knowledge of the schema.

4. **Keep validation loose in contract code.** The contract validates structural integrity (signatures are valid, timestamps are monotonic, status transitions are forward-only). It does NOT validate business rules that might change (price limits, product categories, order policies). Business rules live in the UI.

5. **Push business logic to the UI.** The contract is a CRDT store with access control and merge rules. The UI interprets what the data means. If the UI changes its interpretation, no contract change is needed.

6. **Test merge logic to destruction before launch.** This is the one thing we cannot patch. Fuzz it, property-test it, run adversarial scenarios. A merge bug in production is permanent.

7. **Treat contract code review as a cryptographic ceremony.** The same seriousness as reviewing a Bitcoin consensus change. Multiple reviewers, formal reasoning about edge cases, explicit sign-off. Once deployed, it's carved in stone.

### The upgrade story for breaking changes

If a breaking change is ever truly necessary (critical security fix, fundamental protocol change), the path is a hard fork:

1. Deploy new contracts with new WASM
2. Guardian federation signs a "successor pointer" in old contract state (quorum required — no unilateral takeover)
3. UI reads the successor pointer and switches to new contracts
4. During transition, UI reads from both old and new contracts
5. Users re-register on new contracts (UI automates this)
6. Guardians re-allocate CURD balances on new user contracts

This is expensive, disruptive, and should be treated as a last resort — the same way a Bitcoin hard fork is treated. The goal is to never need it.

### What about the UI?

The UI is a normal web application served by `dx serve` or bundled into a Freenet web container. It can be updated freely — new versions just need to maintain backward compatibility with the deployed contract state format (which `#[serde(default)]` guarantees).

The UI is where most "feature releases" happen. New screens, new workflows, new interpretations of existing data, new business rules — all UI changes, no contract changes needed.

### Summary

| Layer | Mutability | Release cadence |
|-------|-----------|-----------------|
| Contract WASM | Immutable forever | Ship once, get it right |
| Contract state schema | Additive only (soft fork) | Rare, carefully planned |
| UI application | Freely updatable | Normal release cadence |
| Guardian federation | Coordinated upgrade | Rare, ceremony required |

The CREAM contract is a promise: "this logic will always work, and future state may contain fields you don't understand, but you can safely ignore them." Every node on the network can verify that promise because the WASM is immutable. This is the price of decentralization — and the benefit is that no one, including us, can unilaterally change the rules after deployment.

---

## Why can't CREAM delete data?

### The fundamental asymmetry

In a traditional database, deletion is a first-class operation: `DELETE FROM orders WHERE order_id = ?`. Data has a lifecycle — it's created, it lives, it's archived or destroyed. The database is a living thing that forgets.

In Freenet, contract state is append-only by nature. All our merge functions are monotonic — they only grow. This isn't a limitation we chose; it's inherent to decentralized state replication. If node A deletes a record while node B updates it, what happens when they sync? In a centralized database, the transaction log resolves this. In a CRDT, the update wins — because merge must be commutative, associative, and idempotent, and deletion breaks all three unless you add tombstone protocols.

### How CREAM handles "deletion" today

Every "deletion" in CREAM is really a status transition or a soft flag:

| Relational operation | CREAM equivalent |
|---------------------|-----------------|
| `DELETE FROM orders` | `status: Cancelled` or `status: Expired` — the order row stays forever |
| `DELETE FROM products` | Set `quantity_total: 0` or add a `deleted` flag — the product row stays forever |
| `DELETE FROM users` | Not possible — a user contract exists permanently once deployed |
| `DELETE FROM inbox_messages WHERE age > 30d` | `prune_old_messages()` — but this is local only; other nodes may still hold the messages |
| `DROP TABLE storefronts` | Not possible — the contract and its state persist on the network indefinitely |

### Consequences

**State grows forever.** Every order, every wallet transaction, every inbox message is permanent. A storefront contract accumulates its entire lifetime of activity. Freenet imposes a 50 MiB per-contract state limit — this is effectively a hard ceiling on how much business a single storefront can do before it hits the wall.

**The 50 MiB wall is a real design constraint.** Consider: each order is roughly 500 bytes of JSON. A busy supplier processing 50 orders per week would accumulate ~1.3 MB per year of order data alone. Add products, wallet transactions, and the overhead of JSON encoding, and a storefront contract has maybe 10-20 years before it approaches the limit. That sounds comfortable, but it means we cannot be wasteful with state — every field we add, every status transition we record, consumes a non-renewable resource.

**GDPR right to erasure is impossible.** You cannot delete data from a decentralized network. Once state is replicated across nodes, there is no mechanism to ensure all copies are destroyed. This is a fundamental tension between decentralized systems and privacy regulation. For CREAM this is mitigated by the fact that user identity is pseudonymous (ed25519 keys, not real names), but it's worth being honest about.

**No archival strategy.** In a relational system, you'd move completed orders to a history table, archive old transactions, compress cold data. In CREAM, active and historical data share the same contract state, and every `summarize()` and `delta()` call processes all of it. As state grows, sync gets slower.

**Merge conflicts on "deletion" require tombstones.** If we ever implement true deletion (e.g., a supplier removing a product listing), we'd need tombstone records — markers that say "this record was intentionally deleted, don't re-add it on merge." Tombstones themselves are permanent (you can never delete the deletion marker), adding ironic overhead.

### The inbox exception

The inbox contract's `prune_old_messages()` is the one place we fight the append-only nature. It works because:

1. Inbox messages have no cross-references — no other contract state points to a message ID
2. The pruning is deterministic (age > 30 days) so all nodes converge on the same result
3. Messages are keyed by random u64, so there's no risk of ID reuse after pruning

This pattern could theoretically extend to other contracts (e.g., pruning fulfilled orders older than 90 days), but only if we can guarantee no dangling references. An order is referenced by wallet transactions on both sides of the exchange — pruning it would leave orphaned ledger entries pointing at a ghost.

### What the relational model makes obvious

If you model CREAM as a normalized relational database (13 tables, third normal form), deletion is trivial. `CASCADE` handles referential integrity, transactions handle concurrency, and storage is bounded by retention policy. The entire append-only constraint, the tombstone complexity, the 50 MiB wall — all of it exists solely because there is no trusted central server to adjudicate "this record is gone."

This is the most tangible cost of decentralization: your data model loses an entire dimension of expressiveness. INSERT, UPDATE, and SELECT survive the transition to CRDTs. DELETE does not.

---

## What does CREAM's domain model look like in third normal form?

A useful exercise: strip away all the decentralization machinery (contracts, signatures, CRDTs, sync protocol) and model CREAM as a single relational database per co-op. This exposes the pure domain and makes obvious how much of our code exists solely to compensate for not having a trusted server.

### The schema (13 tables, 3NF)

```sql
-- ============================================================
-- CREAM Co-op Database — Third Normal Form
-- ============================================================

-- ----- IDENTITY & USERS -----

CREATE TABLE users (
    user_id         CHAR(64) PRIMARY KEY,       -- ed25519 pubkey hex
    name            TEXT NOT NULL,
    origin_supplier CHAR(64) NOT NULL            -- immutable after first set
                    REFERENCES users(user_id),
    current_supplier CHAR(64) NOT NULL
                    REFERENCES users(user_id),
    invited_by      CHAR(64)
                    REFERENCES users(user_id),
    is_admin        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);
-- balance_curds is NOT stored — it's derived:
--   SELECT COALESCE(SUM(CASE kind WHEN 'credit' THEN amount
--                                  WHEN 'debit'  THEN -amount END), 0)
--   FROM wallet_transactions WHERE user_id = ?


-- ----- STOREFRONTS -----
-- 1:1 with supplier users. The "directory" is just a view over this.

CREATE TABLE storefronts (
    storefront_id   SERIAL PRIMARY KEY,
    owner_id        CHAR(64) NOT NULL UNIQUE
                    REFERENCES users(user_id),
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    latitude        DOUBLE PRECISION NOT NULL,
    longitude       DOUBLE PRECISION NOT NULL,
    postcode        TEXT,
    locality        TEXT,
    timezone        TEXT,                        -- IANA, e.g. 'Australia/Sydney'
    phone           TEXT,
    email           TEXT,
    address         TEXT,
    updated_at      TIMESTAMPTZ NOT NULL
);

-- The current DirectoryEntry.categories is denormalized —
-- it's derivable from the products table:
--
-- CREATE VIEW supplier_categories AS
--   SELECT DISTINCT s.storefront_id, p.category
--   FROM storefronts s
--   JOIN products p ON p.storefront_id = s.storefront_id;


-- ----- OPENING HOURS -----
-- WeeklySchedule bitfield (336 bits) normalized to time ranges.

CREATE TABLE opening_hours (
    storefront_id   INTEGER NOT NULL
                    REFERENCES storefronts(storefront_id),
    day_of_week     SMALLINT NOT NULL CHECK (day_of_week BETWEEN 0 AND 6),
                    -- 0=Monday, 6=Sunday
    open_time       TIME NOT NULL,
    close_time      TIME NOT NULL,
    CHECK (close_time > open_time),
    PRIMARY KEY (storefront_id, day_of_week, open_time)
);


-- ----- PRODUCTS -----

CREATE TABLE product_categories (
    category_id     SERIAL PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE          -- 'Milk','Cheese',...,'Other: xyz'
);

CREATE TABLE products (
    product_id      TEXT PRIMARY KEY,             -- timestamp-based monotonic ID
    storefront_id   INTEGER NOT NULL
                    REFERENCES storefronts(storefront_id),
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    category_id     INTEGER NOT NULL
                    REFERENCES product_categories(category_id),
    price_curd      BIGINT NOT NULL,              -- smallest CURD unit
    quantity_total  INTEGER NOT NULL,
    expiry_date     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);
-- quantity_available is NOT stored — it's derived:
--   quantity_total - SUM(quantity) FROM orders
--     WHERE product_id = ? AND status IN ('reserved','paid')


-- ----- ORDERS -----

CREATE TABLE orders (
    order_id        TEXT PRIMARY KEY,
    product_id      TEXT NOT NULL
                    REFERENCES products(product_id),
    customer_id     CHAR(64) NOT NULL
                    REFERENCES users(user_id),
    quantity        INTEGER NOT NULL,
    deposit_tier    TEXT NOT NULL
                    CHECK (deposit_tier IN ('reserve_2d','reserve_1w','full')),
    deposit_amount  BIGINT NOT NULL,              -- recorded fact, not derived
    total_price     BIGINT NOT NULL,
    status          TEXT NOT NULL
                    CHECK (status IN ('reserved','paid','fulfilled',
                                      'cancelled','expired')),
    expires_at      TIMESTAMPTZ,                  -- only for status='reserved'
    collection_type TEXT
                    CHECK (collection_type IN ('farm_gate','market')),
    collection_market TEXT,                       -- market name if type='market'
    created_at      TIMESTAMPTZ NOT NULL
);
-- deposit_amount: yes it depends on (deposit_tier, total_price), so it's
-- technically a 3NF violation. But it's a recorded fact — what was actually
-- charged — not a derivable attribute. Changing the tier formula later
-- shouldn't retroactively alter historical orders.


-- ----- WALLET -----

CREATE TABLE wallet_transactions (
    user_id         CHAR(64) NOT NULL
                    REFERENCES users(user_id),
    tx_seq          INTEGER NOT NULL,             -- per-user monotonic sequence
    kind            TEXT NOT NULL
                    CHECK (kind IN ('credit','debit')),
    amount          BIGINT NOT NULL,
    description     TEXT NOT NULL,
    counterparty    TEXT NOT NULL,                 -- user name or '__cream_root__'
    tx_ref          TEXT NOT NULL,                 -- links both sides of a transfer
    lightning_hash  TEXT UNIQUE,                   -- prevents double-mint
    created_at      TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, tx_seq),
    UNIQUE (user_id, tx_ref, kind)                -- dedup rule from merge()
);


-- ----- INBOX -----

CREATE TABLE inbox_messages (
    message_id      BIGINT PRIMARY KEY,           -- random u64
    recipient_id    CHAR(64) NOT NULL
                    REFERENCES users(user_id),
    kind            TEXT NOT NULL
                    CHECK (kind IN ('direct','chat_invite',
                                    'market_invite','market_accept')),
    kind_ref        TEXT,                         -- session_id or market_name
    from_name       TEXT NOT NULL,
    from_user_id    CHAR(64)
                    REFERENCES users(user_id),
    body            TEXT NOT NULL,
    toll_paid       BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_inbox_recipient ON inbox_messages(recipient_id, created_at DESC);


-- ----- FARMER'S MARKETS -----

CREATE TABLE markets (
    market_id       SERIAL PRIMARY KEY,
    organizer_id    CHAR(64) NOT NULL
                    REFERENCES users(user_id),
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    venue_address   TEXT NOT NULL,
    latitude        DOUBLE PRECISION NOT NULL,
    longitude       DOUBLE PRECISION NOT NULL,
    postcode        TEXT,
    locality        TEXT,
    timezone        TEXT,
    updated_at      TIMESTAMPTZ NOT NULL
);

CREATE TABLE market_events (
    market_id       INTEGER NOT NULL
                    REFERENCES markets(market_id),
    event_date      DATE NOT NULL,
    start_time      TIME NOT NULL,
    end_time        TIME NOT NULL,
    PRIMARY KEY (market_id, event_date)
);

CREATE TABLE market_suppliers (
    market_id       INTEGER NOT NULL
                    REFERENCES markets(market_id),
    supplier_id     CHAR(64) NOT NULL
                    REFERENCES users(user_id),
    status          TEXT NOT NULL
                    CHECK (status IN ('invited','accepted')),
    PRIMARY KEY (market_id, supplier_id)
);

-- Per-market product selection (which products a supplier brings)
CREATE TABLE market_product_selections (
    market_id       INTEGER NOT NULL,
    supplier_id     CHAR(64) NOT NULL,
    product_id      TEXT NOT NULL
                    REFERENCES products(product_id),
    PRIMARY KEY (market_id, supplier_id, product_id),
    FOREIGN KEY (market_id, supplier_id)
                    REFERENCES market_suppliers(market_id, supplier_id)
);


-- ----- SYSTEM CONFIG -----

CREATE TABLE toll_rates (
    id              INTEGER PRIMARY KEY DEFAULT 1
                    CHECK (id = 1),               -- singleton row
    session_toll_curd     BIGINT NOT NULL DEFAULT 1,
    session_interval_secs INTEGER NOT NULL DEFAULT 10,
    inbox_message_curd    BIGINT NOT NULL DEFAULT 1,
    curd_per_sat          BIGINT NOT NULL DEFAULT 10
);
```

### 3NF violations identified and resolved

| Current CREAM model | Violation | Resolution |
|---|---|---|
| `balance_curds` on UserContractState | Derived from ledger | Computed via `SUM()` |
| `quantity_available` (computed in code) | Derived from orders | Computed via `SUM()` |
| `categories[]` on DirectoryEntry | Derived from products | View with `DISTINCT` |
| `deposit_amount` on Order | Depends on tier + price | Kept — recorded historical fact |

### Structural changes from normalization

- `WeeklySchedule` bitfield → `opening_hours` table with proper time ranges
- `MessageKind` enum with variant data → `kind` column + `kind_ref` for the associated ID/name
- `CollectionPoint` enum → two columns (`collection_type`, `collection_market`)
- `market_products` map nested in StorefrontInfo → junction table `market_product_selections`
- `OrderStatus::Reserved { expires_at }` → separate `status` and `expires_at` columns
- `DirectoryEntry` → just a view over `storefronts` + `products`

### What disappears in the relational model

| CREAM concept | Why it exists | Relational equivalent |
|---|---|---|
| Signatures (ed25519) | No trusted server to authenticate writes | `GRANT`/`REVOKE` + connection auth |
| CRDT merge logic | No central transaction coordinator | SQL transactions with SERIALIZABLE isolation |
| Summarize/delta protocol | Bandwidth-efficient sync between untrusted peers | SQL queries |
| Contract keys (BLAKE3 hashes) | Content-addressed identity on untrusted network | Foreign keys |
| `extra {}` fields (`serde(flatten)`) | Future-proof against immutable contract code | `ALTER TABLE ADD COLUMN` |
| `WeeklySchedule` bitfield | Compact binary for network transfer | Proper time-range table |
| `balance_curds` cached field | Avoid scanning full ledger on every read | `SUM()` or materialized view |

The entire signing infrastructure, the CRDT merge functions, the sync protocol, the `extra` extension fields — all of it exists solely because there is no trusted central server. The actual business logic is just these 13 tables.

---

## How would contract versioning work if we need it?

Freenet contract keys are derived from the hash of WASM code + parameters. **Change the WASM = different key = different contract.** You cannot patch a deployed contract in place. This is by design (immutability is what makes contracts trustworthy), but it means schema upgrades require careful planning.

### Levels of change

| Level | What changes | Example | Upgrade path |
|-------|-------------|---------|-------------|
| 1 — Additive data | New optional fields, new enum variants | Adding `phone` to StorefrontInfo | No WASM change needed. `serde(default)` and `extra` fields handle it transparently. |
| 2 — Logic change | Validation rules, merge behaviour, new contract features | Changing how order status transitions are validated | New WASM required. Same data format, different contract key. |
| 3 — Breaking data | Renamed/removed fields, changed types | Splitting `name` into `first_name`/`last_name` | New WASM + data migration. Hardest case. |

### Current defences (Level 1 — what we do today)

- Every struct has `#[serde(flatten, default)] pub extra: serde_json::Map<String, serde_json::Value>` to capture unknown fields from future versions.
- All new fields use `#[serde(default)]` so old state deserializes cleanly.
- `Option<T>` for fields that may not exist yet.
- This handles additive evolution indefinitely without touching the WASM.

### Migration ceremony (Level 2 and 3 — when WASM must change)

When new contract code is deployed:

1. Deploy the new WASM to the network (creates a new contract key).
2. The UI detects the old contract version on first load after upgrade.
3. Read the user's old contract state via GET.
4. Transform the state to the new format (if needed).
5. PUT it as a new contract instance under the new WASM.
6. Update the directory / any references to point to the new contract key.
7. Old contract eventually gets evicted from the network (nobody reads it).

For the directory (1 instance) this is trivial. For storefronts (1 per supplier) it's manageable — each supplier's UI migrates their own. For user contracts (1 per user) it requires coordinated rollout.

### Thin envelope contracts (future option for non-financial contracts)

For contracts where on-chain validation is less critical (directory, storefronts, inbox, markets), the WASM could be reduced to a minimal envelope:

- Verify the owner's signature.
- Accept the state with the newer timestamp.
- No understanding of the payload structure.

All business logic (product management, order state machines, etc.) moves to the UI layer, which can be updated freely. The contract becomes a signed, versioned blob store whose WASM almost never changes.

The **user contract** (wallet, balances) should NOT use this approach — contract-level validation that you can't mint CURD out of thin air is important. This is the one contract worth investing in careful, future-proof design.

### Recommended strategy (Strategy D — hybrid)

- **User contract**: full on-chain validation. Design very carefully before first production deploy. Use `serde(default)` and `extra` aggressively. Accept that a Level 2 change here requires a migration ceremony.
- **All other contracts**: candidates for the thin envelope approach if/when the upgrade burden becomes painful. Not needed yet — current `serde(default)` + `extra` approach handles foreseeable evolution.
- **Migration ceremony**: build the capability into the UI as latent code. Don't need it on day one, but have the code path ready.

### Alternative considered: guardian-hosted database

Moving non-financial contracts to a relational database (SQLite/Postgres) on the guardian nodes was considered. It would give trivial schema migration via `ALTER TABLE` and rich queryability. However, reliable multi-node replication requires solving the same distributed consensus problems that Freenet contracts already solve (conflict resolution, eventual consistency, leader election). The complexity doesn't disappear — it moves. This remains a fallback option if the contract versioning burden becomes unmanageable.

---
