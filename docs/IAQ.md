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

- **The Fedimint layer protects money regardless.** Even if a supplier manipulates Freenet contract state, e-cash deposits are locked in the federation's consensus. The supplier can't steal funds by tampering with the coordination layer.
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
| **Refund timeliness** | User contract inboxes, Fedimint escrow release timestamps | When orders are cancelled, how quickly do refunds appear? |
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

A curd is a unit of [Fedimint](https://fedimint.org) e-cash. Each curd is backed 1:1 by a Bitcoin satoshi held in the federation's multisig wallet. By holding curds, you are holding Bitcoin — just in a more private and convenient form.

Curds are issued via Chaumian blind signatures: the federation that mints a curd cannot link it back to who received it or trace how it's spent. This is the privacy guarantee that makes CREAM work — participants transact freely without the federation (or anyone else) being able to trace the flow of funds.

### How do you get curds?

Through the Fedimint federation's built-in Lightning gateway:

- **Buy curds (peg-in)**: Send Bitcoin satoshis via Lightning → receive an equal number of curds in your CREAM wallet.
- **Sell curds (peg-out)**: Redeem curds → receive Bitcoin satoshis via Lightning.

This uses Fedimint's standard Lightning module — no custom integration needed. Any Bitcoin Lightning wallet (Phoenix, Muun, Wallet of Satoshi, etc.) can send sats to the CREAM federation and receive curds in return.

### Why curds rather than raw Bitcoin?

Curds provide three things that raw Bitcoin (on-chain or Lightning) doesn't:

1. **Privacy**: Chaumian blind signatures mean the federation cannot trace spending. Bitcoin on-chain transactions are fully traceable; even Lightning has routing metadata. Curds are genuinely private — the federation knows the total amount issued but not who holds what or who paid whom.
2. **Speed and cost**: Curd transfers within CREAM are instant and free — they're just e-cash token exchanges between participants. No on-chain fees, no Lightning routing fees, no channel capacity constraints.
3. **Offline capability**: E-cash tokens are bearer instruments. A payment can be made by handing over the token bytes — no network round-trip to the federation is needed at the moment of payment. The recipient can verify and reissue the tokens later when they're online.

### Curds as network gas

Curds also serve as the **fee currency for the CREAM network**. Every network operation (creating a user contract, listing a product, placing an order, updating a storefront) costs a small amount of curds. This prevents spam and abuse while funding the infrastructure — Fedimint federation guardians and Freenet node operators.

The exact fee schedule and distribution mechanism are yet to be determined, but the fees will be very low — fractions of a cent per operation. The marketplace should feel free to use; fees exist to prevent abuse, not to extract revenue.

### The trust trade-off

The cost of curds' privacy and speed is trust in the Fedimint federation. A threshold of guardian nodes (f < n/3) must remain honest for the system to work. If enough guardians collude, they could theoretically mint unbacked curds or refuse to honour redemptions.

For a local dairy marketplace, this is an acceptable model — especially since the federation could be run by the supplier community itself. The guardians are the farmers. Their reputation, their livelihoods, and their community relationships are at stake. And the amounts involved (dairy product deposits and payments) are small enough that the risk-reward for guardian misbehaviour doesn't make sense.

Since every curd is backed 1:1 by a satoshi, holding curds is economically equivalent to holding Bitcoin. The federation is a privacy layer and convenience layer, not a speculative instrument.

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

CREAM uses a **contract-based escrow** model. The storefront contract — the same Freenet contract that already holds products and orders — also serves as the escrow state machine and communication channel for deposit funds.

### The deposit flow

When a user places an order, they include Fedimint e-cash tokens (curds) as the deposit. The deposit amount is determined by the order's deposit tier (e.g. 10%, 25%, 50%). The tokens are bearer instruments — whoever holds the bytes can redeem them.

1. **User places order**: The user's app mints e-cash tokens via the Fedimint client, includes them in the order as `deposit_tokens: Vec<u8>`, and posts the order to the storefront contract as a state update.
2. **Contract validates**: The storefront contract's `verify_delta()` checks the state transition is legal (new order, valid signature, correct deposit tier). The contract does not understand the e-cash tokens — it just stores the bytes.
3. **Supplier receives tokens**: The supplier's node is subscribed to its own storefront contract. When the update notification arrives, the supplier's node extracts and stores the deposit tokens locally. The supplier now holds the deposit.

At this point, the deposit tokens are in the supplier's possession. What happens next depends on how the order resolves.

### Terminal states and fund flows

An order can reach three terminal states, each with a different fund flow:

#### Fulfilled (happy path)

The user collects their product. The supplier marks the order as `Fulfilled` via a contract update. The user pays the remaining balance (total price minus deposit) in person or via a second e-cash transfer. The supplier keeps the deposit tokens they already hold and redeems them through the Fedimint federation. Everyone is happy.

#### Cancelled (supplier-initiated)

The supplier cannot or chooses not to fulfil the order. The deposit must be returned to the user.

1. **Supplier updates the storefront contract** transitioning the order to `Cancelled`.
2. **Supplier writes refund tokens to the user's contract** — because every transacting participant has a user contract on the network, the supplier can deliver the refund directly to the user's inbox rather than stuffing encrypted blobs into the storefront. The user's app is subscribed to its own contract and receives the refund immediately.
3. **Fedimint escrow releases automatically** — on the money layer, the federation releases the locked deposit back to the user's e-cash (see below).

With the dual-layer architecture, the Freenet-side refund tokens serve as a backup communication channel. The Fedimint escrow is the authoritative fund release — it doesn't require trust because the federation held the funds from the start.

#### Expired (user no-show)

The user reserved product, the supplier held it aside, and the user never collected within the reservation window. The supplier is owed the deposit as compensation for the opportunity cost of holding inventory.

On the Freenet layer, the contract transitions to `Expired` (the supplier's node runs the expiry check, already implemented in `node_api.rs`) and the held product is released back to available inventory. On the Fedimint layer, the supplier claims the locked deposit from the federation after the timeout. No refund is posted.

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

**Neither Freenet contract enforces fund movement** — that's the Fedimint escrow's job. The Freenet layer coordinates; the Fedimint layer moves money. Together, the dual-layer architecture eliminates the trust gap: the Fedimint federation holds deposits from the moment an order is placed, so supplier insolvency is not a risk and cancellation refunds are automatic.

### Why the trust model works

With the dual-layer architecture (Freenet contracts + Fedimint escrow), trust requirements are minimal:

- **Fulfilment**: No trust needed. User pays, gets product. Supplier gets paid. Both parties are satisfied before the transaction completes.
- **Expiry**: No trust needed. Fedimint federation releases locked deposit to supplier after timeout.
- **Cancellation**: No trust needed. Fedimint federation releases locked deposit back to user automatically. The supplier doesn't need to have funds — the federation already has them.

---

## How do the Freenet and Fedimint "contracts" work together for escrow?

Both Freenet and Fedimint use the word "contract", but they mean fundamentally different things. CREAM uses both in parallel for every transaction — they serve complementary roles.

### What "contract" means in each system

A **Freenet contract** is a passive data container with validation rules. It defines a chunk of state that lives on the network, plus functions that accept or reject proposed changes (`verify_delta()`), merge updates (`update_state()`), and summarise state for sync. The contract code runs on hosting nodes when someone proposes a state change — it can say "yes, this transition is valid" or "no, reject it." But it cannot initiate anything, hold funds, execute logic on a timer, or interact with external systems. It's a gatekeeper, not an actor.

A **Fedimint contract** is a conditional fund lock inside the federation's consensus. It lives within a Fedimint transaction as inputs and outputs. A contract says: "these funds are locked, and here are the conditions under which they can be released." The federation's consensus engine — a quorum of guardian nodes running AlephBFT — actively evaluates the conditions and releases funds when they're met.

The Lightning module gives the clearest example: an `OutgoingContract` locks e-cash and says "release these funds to the gateway when it provides the preimage for this payment hash." The federation guardians actively verify the preimage and execute the release. The contract is *enforced* by the consensus, not just *validated*.

### The dual-layer architecture

CREAM runs both mechanisms for every order. They are not alternatives — they are complementary layers:

- **Fedimint escrow** is the money layer. A custom Fedimint module locks the deposit in the federation's consensus at order time. The funds are genuinely held — the supplier cannot spend them before fulfilment or expiry, and the user gets them back automatically on cancellation. No trust required. No insolvency risk.

- **Freenet contracts** are the coordination layer. The storefront contract tracks order status, product listings, and state transitions. User contracts provide a persistent inbox for each participant — a direct channel for refund delivery, messages, and notifications. Together they handle all the coordination that doesn't involve moving money.

Both layers process the same events (order placed, fulfilled, cancelled, expired) but each handles the aspect it's good at. The Freenet contracts coordinate; the Fedimint escrow moves money.

### The Fedimint escrow module

CREAM includes a custom Fedimint module (following the three-part `common`/`client`/`server` pattern) that defines an escrow contract type:

1. **User places order**: A Fedimint transaction locks the deposit amount into an `EscrowOutput` with conditions — release to supplier on fulfilment or expiry, release back to user on cancellation. Simultaneously, the order is posted to the Freenet storefront contract.
2. **Federation enforces**: The guardian nodes hold the locked funds in consensus. When someone submits an `EscrowInput` claiming the funds, the server module's `validate_input()` checks whether the claim satisfies the conditions. If yes, the funds move. If no, rejected.
3. **Resolution**: Whoever meets the conditions submits a transaction to claim. The federation executes it atomically — funds move from the escrow to the claimant's e-cash notes in a single consensus round. The Freenet contract records the terminal state.

### Dual wallets and reconciliation

Each CREAM client maintains two wallet views:

- **Freenet-side wallet**: Tracks the token flows visible across the user's contract and storefront contracts — deposits posted with orders, refund notifications received, balances derived from the transaction ledger.
- **Fedimint-side wallet**: Tracks the escrow locks and releases managed by the federation — deposits locked, funds claimed on fulfilment/expiry, refunds released on cancellation.

These two views should always agree. If they diverge — say the Freenet contract shows an order as Fulfilled but the Fedimint escrow hasn't been claimed yet — something has gone wrong and the discrepancy points directly to the problem. This built-in reconciliation is invaluable for debugging a system spanning two decentralised networks.

### How the terminal states work across both layers

#### Fulfilled (happy path)

| Layer | What happens |
|-------|-------------|
| Freenet | Supplier updates order status to `Fulfilled` |
| Fedimint | Supplier submits `EscrowInput` with fulfilment proof → federation releases deposit to supplier's e-cash |

#### Cancelled (supplier-initiated)

| Layer | What happens |
|-------|-------------|
| Freenet | Supplier updates order status to `Cancelled` in storefront; writes refund notification to user's contract inbox |
| Fedimint | Supplier submits `EscrowInput` with cancellation signature → federation releases deposit back to user's e-cash |

The user doesn't need to trust the supplier — the federation holds the funds and releases them automatically. The supplier's insolvency is irrelevant because the deposit was locked before the supplier ever saw the order. The Freenet-side notification to the user's contract is a courtesy — the Fedimint escrow is the authoritative fund release.

#### Expired (user no-show)

| Layer | What happens |
|-------|-------------|
| Freenet | Order transitions to `Expired`; held product released back to inventory |
| Fedimint | Expiry time passes → supplier submits `EscrowInput` with timeout proof → federation releases deposit to supplier's e-cash |

### Binary size impact

The Fedimint client SDK is already a required dependency for CURD wallet functionality (minting, redeeming, peg-in/peg-out via the mint module). The custom escrow module adds minimal overhead on top of this:

- A minimal Fedimint module following the `fedimint-dummy-client` template is ~400-800 lines of client code + ~100-200 lines of common types.
- It reuses cryptographic dependencies (ed25519, BLS12-381) already pulled in by the mint module.
- Estimated binary impact: ~30-50 KB uncompressed on a WASM binary that already includes `fedimint-client` + mint.

The heavy cost (Fedimint client SDK + crypto) is paid once for the wallet. The escrow module rides alongside for near-zero marginal cost.

### What each layer enforces

| | Freenet contracts | Fedimint escrow |
|---|---|---|
| **Valid state transitions** | Yes — `verify_delta()` rejects illegal status changes | Yes — `validate_input()` rejects invalid claims |
| **Correct signatures** | Yes — only supplier/user can update their fields | Yes — claim conditions are cryptographically verified |
| **Actual fund custody** | No — can't enforce fund movement | Yes — federation holds and releases real funds |
| **Insolvency protection** | No — not its job | Yes — funds locked upfront, always available |
| **Communication channel** | Yes — storefronts for orders, user contracts for direct delivery | No — only fund locks and releases |
| **Participant reachability** | Yes — user contracts make everyone contactable | No — federation doesn't route messages |
| **Works offline/degraded** | Yes — Freenet state persists on hosting nodes | Requires federation quorum to be reachable |

---
