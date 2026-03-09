# CREAM FAQ

> **CREAM rises to the top**  
> **Byline:** the decentralized, private 24/7 farmer's market  
> **Tagline:** sats – soup to nuts, but cash when you want to.

---

## What is CREAM?

CREAM (Cream Retail Exchange and Marketplace) is a new kind of farmer’s market that runs on decentralized technology instead of a central website or company.

It is designed for people who care about:

- Local, high‑quality food.
- Access to products that are hard to find through official channels (like raw dairy in some jurisdictions).
- Privacy, cash options, and direct relationships with producers.

You can think of it as “an always‑on, invite‑only farmer’s market that lives on your devices and in your local community,” rather than at a single stall on Saturday morning.

---

## Why does CREAM exist?

In many places, it’s hard or impossible to buy certain kinds of food (like raw milk and raw dairy products) at normal shops or even farmer’s markets, even though there is clear demand from informed adults.

At the same time:

- Small producers struggle to connect with the right buyers.
- Buyers want to know *who* they are buying from, and how the food is produced.
- More and more people want payment options that aren’t just cards and bank apps.

CREAM exists to:

- Help local producers and buyers find each other.
- Make small‑scale, trust‑based trades easier to coordinate.
- Support private, low‑friction payments — including cash in person.

---

## Is CREAM a website or an app?

From a user’s perspective, CREAM looks like an app.

Under the hood, it is **not** a normal centralized website. Instead, it runs on a decentralized network (Freenet) that does not rely on any single company, server, or country.

That means:

- There is no central “CREAM server” that can be turned off.
- Listings, messages, and contracts are spread across many nodes.
- Participation is more like joining a network than signing up at a website.

You still use a normal device (phone, laptop, etc.), but what happens behind the scenes is very different from the usual “log into a platform” model.

---

## What kinds of products is CREAM for?

CREAM is designed first and foremost for **real, physical produce**, especially:

- Raw dairy (milk, cream, butter, cheese) where legal or tolerated.
- Eggs, meat, honey, fruit, vegetables, herbs.
- Value‑added products (ferments, broths, etc.) at the discretion of local communities.

Digital goods are not the focus. CREAM is about food that comes from land, animals, and people you can meet.

---

## Is this legal?

Short answer: Who cares?

CREAM is a coordination tool. It helps people:

- Discover each other.  
- Share information about products and pickup times.  
- Arrange deposits and payments.  

Each producer and buyer is responsible for knowing and following the laws and regulations in their own area.

CREAM does not:

- Act as a legal intermediary.  
- Provide legal advice.  
- Guarantee the legality of any particular product in your jurisdiction.  

Local communities may choose to use CREAM in ways that are fully above‑board, somewhat “grey”, or strictly private. CREAM is designed to support that range, but it does not tell anyone what to do.

## Why does CREAM care so much about cash?

Because cash:

- Is simple and universal.
- Leaves no digital trace between buyer and seller.
- Fits naturally with face‑to‑face, local food exchanges.

CREAM encourages flows like:

1. Reserve a product on line (so the farmer knows you’re serious).
2. Meet in person.
3. Pay in cash if you prefer, or complete payment digitally if that suits both sides.

Digital money is used mainly for coordination and deposits. Cash stays a first‑class citizen at the point of hand‑over.

---

## What does “sats – soup to nuts, but cash when you want to” mean?

It means:

- Under the hood, CREAM runs on **Bitcoin**, specifically tiny units called **sats** (satoshis).
- Inside CREAM, sats are held and moved using a community e‑cash called **CURDs**.
- At the final step, buyer and seller can decide to settle in:
  - Cash, in person.
  - CURDs (the internal e‑cash).
  - Or some mix of both.

So from the first moment you join CREAM, all the internal accounting is Bitcoin‑based (“sats – soup to nuts”), but the system never forces you to abandon physical cash (“cash when you want to”).

---

## What are CURDs?

CURDs stands for **Completely Unstoppable Raw Dairy** — a playful name for the e‑cash used inside CREAM.

Technically:

- CURDs are **Bitcoin‑backed e‑cash** issued by CREAM’s guardian federation.
- A *guardian federation* is a group of trusted community members who collectively hold Bitcoin via threshold cryptography (FROST) and issue private e‑cash tokens against it.
- Users deposit BTC to the federation (via Lightning) and receive CURDs; they can later redeem CURDs back into BTC.

Practically, as a user:

- CURDs feel like digital “credits” you can use for:
  - Micro‑payments inside CREAM (fees, small tips, etc.).
  - Reservation deposits.
  - Full or partial payment for goods if both sides agree.
- CURDs are designed to be **private** (Chaumian e‑cash) and fast.

---

## Why use CURDs at all if we have cash?

Because cash alone cannot do everything we need:

- Producers want some assurance that a buyer will actually show up for perishable items.
- Some buyers and sellers prefer to complete payment digitally for convenience.
- The network itself (CREAM) needs a way to pay for its own resources.

CURDs solve these problems by acting as:

- **Gas:** tiny CURD fees that keep the system sustainable and prevent spam.
- **Deposits:** amounts locked as a promise to complete a trade.
- **Optional settlement medium:** for those who want to stay digital all the way.

Cash is for the moment when hands meet. CURDs are for everything around that moment.

---

## How do deposits work in CREAM?

At a high level:

1. A producer lists a product (for example, 2 litres of raw milk for pickup on Saturday morning).
2. The listing specifies whether a **deposit in CURDs** is required.
3. A buyer places an order and locks the deposit amount:
   - CREAM creates a **contract** that describes the reservation (who, what, when).
   - On the e‑cash side, CURDs are locked in a way that they cannot be spent elsewhere during the reservation.
4. At pickup, there are two main options:
   - Buyer pays in cash → the deposit is returned in CURDs, or partly converted as agreed.
   - Buyer pays in CURDs → the deposit is applied to the final amount.
5. If the buyer fails to show up (subject to agreed rules and timeouts), the deposit may go to the producer as compensation.

The exact rules can be tuned by communities and producers, but the general idea is: **deposits align incentives without needing heavy‑handed enforcement**.

---

## Who runs the CURDs federation?

CREAM is designed so that **local communities** can run their own federations.

Typically, guardians might be:

- Local farmers or producers.
- Trusted community members.
- Technically competent people chosen by the group.

Each federation holds Bitcoin and issues CURDs within its own circle of trust. Different CREAM communities may choose different guardian sets. This keeps trust local and transparent, rather than centralized in a distant company.

---

## How private is CREAM?

CREAM aims for strong, practical privacy:

- The underlying network (Freenet) avoids central servers and routes data in a way that does not easily reveal who is talking to whom.
- CURDs use a **privacy‑preserving e‑cash** design, which hides who paid whom, while still enforcing balances and preventing double‑spends.
- Cash in person has no digital trail at all.

That said:

- No system is perfect.
- Device security, behaviour, and local laws still matter.
- Users should treat CREAM as a powerful privacy‑enhancing tool, not a magic invisibility cloak.

---

## I’m not a Bitcoiner. Can I still use CREAM?

Yes, if your local community chooses to help you on‑board.

Possible approaches:

- A trusted guardian or local member can act as a “gateway” to convert your cash into CURDs and back again.
- Over time, you may choose to learn enough Bitcoin basics to hold and manage sats directly, but it is not required on day one.

The design intention is that **farmers and families** can use CREAM with help from people they already know and trust, rather than having to become Bitcoin experts overnight.

---

## What if I just want to buy food and stay out of the tech?

That is completely fine.

CREAM is built so that:

- You can treat it mostly as a **private noticeboard and reservation tool**.
- You can hand over physical cash at pickup and ignore almost everything about Bitcoin and CURDs if you want to.
- The more technical parts can be handled by:
  - Guardians.
  - Local “power users”.
  - Tools and interfaces that simplify the underlying complexity.

The aim is to keep the *human* experience simple: know your farmer, reserve what you need, show up, pay, go home with good food.

---

## How do I get involved?

That will depend on where CREAM is in its rollout.

In general:

- As a buyer:  
  Join a local CREAM community (this might start as an invite link, a QR code, or a simple introduction from a friend), then learn the basic flow: find producers, reserve products, pick up, pay.

- As a producer:  
  Get set up with a CREAM‑compatible app or node, define your products, pickup times, and deposit rules, and decide whether you prefer cash, CURDs, or both.

- As a guardian:  
  Help form or join a local federation that will issue CURDs, facilitate onboarding for non‑technical users, and maintain local norms around what is offered and how disputes are handled.

More concrete “how‑to” steps, screenshots, and guides will be added as the project matures.

## Who and what are CREAM guardians?

CREAM guardians are the initial operators of the CREAM network. Each guardian runs a CREAM node that is also a full Bitcoin node and a Lightning gateway, so they directly participate in both on‑chain and off‑chain settlement.

There are initially three guardians. They jointly assist with signing transactions on behalf of the network wherever funds need to be controlled collectively, such as when deposits are held in escrow to be released either to the supplier on fulfilment or back to the customer on cancellation.

Guardians use a threshold‑signing protocol called FROST (Flexible Round‑Optimized Schnorr Threshold), which means no single guardian ever holds or can use the full private key. As long as a majority of guardians are online and behaving honestly, the network can continue to operate safely: required signatures can be produced, but no minority of guardians can unilaterally steal or misdirect funds.

In the future, CREAM may optionally support Fedimint e‑cash wallets for communities that prefer that model, but this is not required. CREAM's native FROST‑based guardian federation provides all the same guarantees — threshold custody, escrow, and Lightning settlement — without the additional dependency.

---

## What is CREAM‑lite?

CREAM‑lite is a lighter way to run CREAM for communities that already use **Fedimint**.

With the full version of CREAM, your community runs its own guardian federation (FROST threshold signing, CURD e‑cash, Lightning gateway — the works). That gives you maximum independence, but it also means setting up and maintaining those guardians yourself.

CREAM‑lite takes a different approach: instead of running its own e‑cash layer, it plugs into an **existing Fedimint federation** that your community already trusts. CREAM provides the marketplace — product listings, orders, pickup coordination, all running on Freenet — while Fedimint handles the money side (e‑cash issuance, custody, and Lightning).

For escrow (holding deposits until an order is fulfilled or cancelled), CREAM‑lite uses a custom **Fedimint escrow module** that locks and releases funds within the federation. The marketplace tells the federation "hold these funds for this order," and later "release them to the supplier" or "refund the customer." The federation's existing threshold consensus ensures no single party can steal the deposit.

### Who is CREAM‑lite for?

- Communities that already run a Fedimint federation and want to add a marketplace.
- Groups that want CREAM's privacy and decentralisation but don't want to stand up their own guardian infrastructure from scratch.
- Early adopters who want to try CREAM with minimal setup — just Freenet nodes and an existing Fedimint.

### Do I have to choose one or the other?

No. A community could start with CREAM‑lite (riding an existing Fedimint) and later migrate to the full CREAM guardian setup if they want more independence. Or they could stay on CREAM‑lite permanently — it depends on what suits the community.

---

## Can CREAM run without Freenet?

Yes. CREAM ships a lightweight alternative backend called **cream‑node** — a single‑process server that speaks the exact same WebSocket protocol as a Freenet node but stores contract state in Postgres instead of a distributed hash table.

Because the wire protocol is identical (bincode over WebSocket, same `ClientRequest` / `HostResponse` types), the UI connects to cream‑node with **zero code changes** — it just points at a different port.

cream‑node is useful in three scenarios:

1. **Development and testing** — no 7‑node Freenet cluster to start, no network propagation delays, no flakiness from distributed consensus. Tests run in seconds instead of minutes.
2. **CREAM‑lite single‑supplier deployments** — a farmer who just wants to run their own online storefront can run cream‑node + the guardian on a single machine (or a Start9 server) without needing Freenet at all.
3. **Auditing and compliance** — cream‑node maintains an immutable audit trail of every contract state change, which Freenet does not provide.

The same contract validation logic runs in both modes — cream‑node calls the native Rust merge/validate methods from `cream-common` directly, bypassing the WASM sandbox but producing identical results.

---

## What is Time Travel in CREAM?

Every time a contract's state changes (a supplier updates a product, an order status advances, a CURD transfer is recorded), cream‑node writes an immutable entry to an **audit log** in Postgres. These entries can never be modified or deleted.

This means you can **reconstruct the state of any contract at any point in the past** — what products were listed last Tuesday, what an order's status was before it was cancelled, what a user's CURD balance was at midnight.

This is useful for:

- **Dispute resolution** — proving what was agreed, what was paid, and when.
- **Debugging** — replaying the exact sequence of state changes that led to a problem.
- **Accountability** — an auditor can verify that the system behaved correctly by examining the full history.

Time travel is only available on cream‑node (the Postgres backend). Freenet's distributed storage does not maintain history.

---

## What CREAM is *not*

- It is not a get‑rich‑quick token or speculative scheme.
- It is not a centralized marketplace company.
- It is not trying to replace cash or real‑world relationships.

CREAM is a tool for communities that already value:

- Real food.
- Real relationships.
- Real privacy.

If that’s you, you’re exactly who this is for.
