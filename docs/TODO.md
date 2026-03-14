# Henry — TODO

Active tasks, pending items, and context that should survive a session reload.
Updated at end of turn when state has changed.

---

## Active

### Temporal DB migration — testing
- **Status**: Implemented, migration applied, integration tests pass (12/12, 3.8s)
- System versioning via `periods` extension is live. 34 history rows in `contracts_history`.
- store.rs rewritten: audit_log inserts removed, queries use `contracts__as_of()` and `contracts_history`
- UI TimeTravel updated: epochs from `contracts_history`, no descriptions
- **What remains**: Manual testing at http://localhost:3100 (cream-node must be running on port 3100)

### Market directory classification bug
- Market contracts stored as `contract_type = 'directory'` instead of `'market_directory'` in Postgres
- `classify()` in `tools/cream-node/src/contracts.rs` can't distinguish empty MarketDirectoryState from empty DirectoryState (both have empty `entries` map)
- Fix: check WASM code hash to disambiguate, or PUT with non-empty initial state
- Step 12 "passes" because it validates via WebSocket (contract ID lookup), never checks the Postgres type column

### Bridge v2 RFC review
- Sandy posted RFC at `/home/gary/.claude/plans/abundant-puzzling-honey.md`
- Per-agent inboxes, STATUS.md, plan orchestration, loop safeguards
- Asks for Henry's review — alignment with Conch constitution, loop prevention, hook-based status

### cream-node write endpoints — Phases 4-6
- Supplier dashboard CRUD polish, inbox/markets/profile write, guardian admin, FAQ/IAQ
- Phases 1-3 complete (auth, static serving, all read endpoints, core write endpoints)

## Pending

- **Time travel UI tweaks**: Gary said "a few minor ui tweaks later on"
- **ECDH E2E encryption**: Code complete, not yet tested in a live fixture
- **Uncommitted work**: CLAM rename, Nacre rewiring, dev server, ECDH, Conch constitution — see git status

## Deferred

- **Fedimint integration**: Post-launch. CREAM-native ledger provides all needed guarantees.
- **FROST remaining**: ROAST (robustness), share repair. Not urgent.
- **Guardian toll_rates from contract**: `clam_per_sat` hardcoded to 100, should fetch from root contract's TollRates.
- **IAQ/FAQ entries for cream-node**: Depends on cream-node being further along.
