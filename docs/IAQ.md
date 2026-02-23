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

A user is anyone who has created a **user contract** on the Freenet network. This contract is their identity and presence on the network — it makes them permanently contactable. The user contract holds:

- **Public key** — the user's ed25519 identity
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

### Privacy considerations

When a guest connects to a supplier's node, that supplier can see the guest's requests. In the context of a local dairy marketplace, where you're already showing up at someone's farm to collect raw milk, this is an acceptable trade-off.

Once a guest becomes a user, their user contract is on the network and their interactions flow through it. The user subscribes to their own contract and to any storefronts they're interested in. Traffic still routes through whatever node they're connected to, but their identity is now network-persistent rather than session-ephemeral.

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

## How does CREAM handle currencies?

CREAM allows each user to configure their wallet to work in one of three currencies: **cents**, **sats**, or **curds**.

| Currency | What it is | Representation |
|----------|-----------|----------------|
| **cents** | Australian fiat currency (AUD) | Integer number of cents (e.g. 450 = $4.50) |
| **sats** | Bitcoin satoshis | Integer (1 BTC = 100,000,000 sats) |
| **curds** | Fedimint e-cash tokens | Integer (1 curd = 1 sat at present) |

The default currency is **curds**. A user who only browses CREAM — viewing suppliers, checking prices — is never required to purchase curds. The currency choice only matters when money changes hands.

### How curds work

Curds are units of e-cash created by a [Fedimint](https://fedimint.org) federation. Each curd is currently worth exactly one satoshi, though this peg may change in the future. Curds provide the privacy guarantees of Chaumian blind signatures: the federation that issues a curd cannot link it back to who received it or trace how it's spent. All internal CREAM transactions — order deposits, payments, refunds — are conducted in curds.

### Supplier currency choice

A supplier can run their wallet in any of the three currencies. This affects how they see prices and balances:

- **curds** (default): Prices and balances displayed in curds. No conversion needed — this is the native internal currency.
- **sats**: Prices and balances displayed in satoshis. Since 1 curd = 1 sat at present, this is currently a no-op conversion, but exists as a distinct option for when/if the peg changes.
- **cents**: Prices and balances displayed in AUD cents. Incoming payments (order deposits, etc.) are converted from curds to cents at the current BTC/AUD exchange rate, sourced from a reputable provider (e.g. CoinGecko, Coinbase). The supplier sees stable dollar amounts rather than volatile crypto values.

### User currency choice

Users make the same choice. If a user selects **cents**, amounts shown in the UI are converted from the underlying curd values to AUD cents at the current exchange rate. The user thinks in dollars; the network transacts in curds.

### How conversions work

All transactions on the CREAM network are internally conducted in curds. Currency selection is a **display and conversion layer** at each end of a transaction:

1. User sees a product priced at 450 cents ($4.50 AUD).
2. At order time, CREAM converts 450 cents → equivalent curds at the current BTC/AUD rate.
3. The order deposit is transmitted as curds through the Freenet network.
4. The supplier receives curds. If they've selected "cents", their wallet converts the received curds back to cents for display.

The exchange rate is fetched from an external price feed at transaction time. Both parties see amounts in their chosen currency; the network only ever moves curds.

### Curd ↔ sat conversion

Conversions between curds and sats are handled via the Fedimint federation's built-in **Lightning gateway**. A user can:

- **Buy curds**: Send sats via Lightning → receive curds (peg-in).
- **Sell curds**: Redeem curds → receive sats via Lightning (peg-out).

This uses Fedimint's standard Lightning module — no custom integration needed.

### Curd/sat ↔ cents conversion

Converting between crypto and fiat is the hardest problem. At present, CREAM does not include a built-in fiat on/off-ramp. The "cents" currency option is purely a **display convenience** — it shows fiat-equivalent values based on the current exchange rate, but the user still holds curds underneath.

How a user actually acquires or redeems AUD is left as a future problem. Possible approaches include integration with a Bitcoin/AUD exchange, peer-to-peer trading, or simply accepting that suppliers who choose "cents" are using it as a mental accounting tool while actually transacting in crypto.

### Curds as network gas

Curds also serve as the **fee currency for the CREAM network** — a sort of gas that pays for usage. Every network operation (listing a product, placing an order, updating a storefront) costs a small amount of curds. This prevents spam and abuse while funding the infrastructure (Fedimint federation guardians, Freenet node operators, etc.).

The exact fee schedule and distribution mechanism are yet to be determined, but the fees will be very low — fractions of a cent per operation. The marketplace should feel free to use; fees exist to prevent abuse, not to extract revenue.

### Why curds rather than raw sats?

Curds provide two things that raw Bitcoin satoshis don't:

1. **Privacy**: Chaumian blind signatures mean the federation cannot trace spending. Bitcoin on-chain transactions are fully traceable; even Lightning has routing metadata. Curds are genuinely private.
2. **Speed and cost**: Curd transfers within CREAM are instant and free — they're just e-cash token exchanges. No on-chain fees, no Lightning routing fees, no channel capacity constraints.

The trade-off is trust in the Fedimint federation (a threshold of guardians must remain honest), but for a local dairy marketplace this is an acceptable model — especially since the federation could be run by the supplier community itself.

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
