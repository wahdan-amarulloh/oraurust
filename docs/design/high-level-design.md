# oraurust — High-Level Design

- **Status:** Draft v0.3 (2026-11-23)
- **Based on:** repo at `d610946` (PR #12), `GLOSSARY.md`, ADR-0001, spec issue #1 and open tickets #6–#11. Items marked **(beyond spec)** are proposals that need a spec update or a new issue.
- **Vocabulary:** follows `GLOSSARY.md`, with one proposed change (see §9).

## 1. Problem & scope

A native Windows desktop client for Oracle SQL/PL/SQL, built in Rust on GPUI (gpui-kit).

**Core features**
1. Author and run scripts (SQL and PL/SQL blocks) with highlighting and completion.
2. Explore the schema and view object DDL/source.
3. View results in a virtualized grid, export CSV, and edit data through a Data Grid.
4. Manage connections: saved profiles and multiple editor tabs (spec stories 1, 3). **(beyond spec)** TNS aliases and multiple concurrent sessions.

**Non-functional**
| Quality | Target |
|---|---|
| UI responsiveness | input → paint < 16 ms; the UI thread never blocks on Oracle |
| Execute feedback | "Executing…" visible < 100 ms after Run |
| Completion | < 50 ms p95 from the in-memory Catalog |
| Memory | result rows held per grid capped (~100 MB); larger results stream to CSV |
| Transactions | DML is never auto-committed; explicit Commit/Rollback per session |
| Security | passwords sealed with DPAPI; never logged; never plaintext on disk |
| Data loss | editor text survives a crash (autosave; beyond spec) |
| Platform | Windows 10+ 64-bit only |

**Out of scope (spec):** stored procedure debugger, explain plan, SQL history, schema compare, session/trace monitoring, multiple result sets per script/tab, thin-driver migration, non-Windows platforms, installers/auto-update/settings sync.

**Also out (this design):** wallet/SSL management, SYSDBA, SSH tunnels, DRCP pooling.

**Assumptions:** Oracle Instant Client 11.2+ (64-bit) installed by the user (ADR-0001); `tnsnames.ora` lives in `TNS_ADMIN` or `<instantclient>\network\admin`; at most ~8 concurrent sessions.

## 2. Scale estimates

A single-user desktop app: the load is data volume, not QPS.

| Quantity | Number (assumption) | What it forces |
|---|---|---|
| Statements | ≤ 1/s, human-driven | no queueing infrastructure |
| Memory per cell as `Option<String>` | ~48 B | `execute()` currently materialises all rows |
| 100k rows × 20 cols | ~100 MB | cap the window held in the grid |
| 1M rows × 20 cols | ~1 GB | stream to CSV, never hold in memory |
| Fetch rate at 20 ms RTT | ~5k rows/s at array size 100; ~25k rows/s at 500 | latency-bound: use `fetch_array_size` 500–1000 |
| Sessions | ≤ 8 | one worker thread each is cheap; cap total grid memory across sessions |
| Frame budget | 16 ms | the grid renders only visible rows |

## 3. Interfaces

UI ↔ worker, per session (bounded channels):

```rust
enum Command { Connect(Profile), Execute{id, sql, page}, FetchMore{id, n}, Cancel(id),
               Commit, Rollback, LoadCatalog, FetchDdl(Object), Disconnect }
enum Event   { Connected, Rows{id, cols, page, done}, RowCount{id, n}, Ddl{id},
               Output{id, lines}, Problems{id, errs}, Failed{id, DbError}, SessionLost(DbError) }
```

Session states: `Disconnected → Connecting → Idle(clean|dirty) → Executing → Lost`.

**Seam note.** The spec fixes `Connection` at four methods (`execute`, `describe_schema`, `fetch_ddl`, `fetch_source`) and names it as a tested boundary. Commit/rollback can go through `execute("COMMIT")` / `execute("ROLLBACK")` (ticket #7), with the dirty flag derived from outcomes (DML row count → dirty; DDL, commit, rollback → clean). Cancel cannot, because `execute` is blocked: it needs a separate `CancelHandle` that calls `break_execution`. Paged fetch changes `StatementOutcome::Rows` from a full `ResultSet` to a first page plus a cursor. **Decided in [ADR-0002](../adr/0002-session-worker-cancel-paged-fetch.md)** (issue #6 shipped): four-method seam, separate `CancelHandle` obtained from the concrete driver, a `Session` worker thread, and 500-row pages capped at 10,000 rows. **Transactions and CSV export are decided in [ADR-0003](../adr/0003-transactions-and-csv.md)** (issue #7 shipped): COMMIT/ROLLBACK as statements, the dirty rule in core (`TransactionEffect`/`TransactionState`), and a pure `csv::to_csv`.

Profile target (new, in core):

```rust
enum Target { EasyConnect{host, port, service}, TnsAlias(String) }
```

`Profile::connect_string()` returns `//host:port/service` or the alias name. Aliases are resolved by the Oracle client, not by us. Core adds a pure `tns` module (`list_aliases(dir)`) used only to populate an alias picker.

## 4. High-level design

```
 UI — GPUI main thread (never blocks on Oracle)
 ┌────────┬────────────┬───────────────┬──────────────────┐
 │ Editor │ Connection │ Schema Browser│ Result/Data Grid │
 │  tabs  │   Panel    │ + DDL Viewer  │ + Output/Problems│
 └───┬────┴─────┬──────┴───────┬───────┴────────┬─────────┘
     └──────────┴── commands ──┴──── events ────┘
                       │              ▲
 ┌─────────────────────▼──────────────┴─────────────────────┐
 │ SessionManager (GPUI Entity)                             │
 │ SessionId -> { state, txn dirty, queue, Catalog }        │
 └───┬─────────────────┬───────────────────────┬────────────┘
     │ pure logic      │ files                 │ Command / Event
     ▼                 ▼                       ▼
 ┌─────────────────┐ ┌──────────────────┐ ┌───────────────────────────┐
 │ oraurust-core   │ │ Local Store      │ │ DB Worker thread  x N     │
 │ ScriptSplitter  │ │ %APPDATA%\       │ │  OraConnection            │
 │ Catalog, tns    │ │  oraurust\       │ │  impl Connection (trait)  │
 │ DataGridMutator │ │ profiles (DPAPI) │ │  oracle 0.6.3 / ODPI-C    │
 │ ProfileStore    │ │ logs, drafts     │ └─────────────┬─────────────┘
 └─────────────────┘ └──────────────────┘               │ OCI
                                          Instant Client (oci.dll, TNS_ADMIN)
                                                        │ SQL*Net :1521
                                          Oracle Database 11g–23ai
```

| Component | Status | Why it exists |
|---|---|---|
| Editor / Connection panels | exists | author scripts; manage profiles |
| Schema Browser + DDL Viewer, Result/Data Grid | planned (#8, #6) | explore and view data |
| Session (becomes SessionManager) | exists (#6) | one entity owns the worker and state instead of `ConnectionPanel`; becomes a manager only when multi-session lands |
| DB Worker thread (one per session) | exists (#6) | serialises OCI calls, holds the open cursor; cancel via a separate handle (ADR-0002) |
| `Connection` trait | exists | the seam; `OraConnection` (prod), `MockConnection` (tests) |
| `ScriptSplitter` | exists | script text → statements |
| Catalog Cache | planned | completion and Schema Browser, per session |
| `DataGridMutator` | planned | builds `UPDATE … WHERE ROWID = :rid` with bind variables |
| `ProfileStore` | exists, extend | add `Target`, keep DPAPI sealing |
| `tns` module | **new** | list aliases from `tnsnames.ora` |

## 5. Data model

- `profiles.json` (exists): one entry per profile, keyed by unique `name`, password DPAPI-sealed. Add a `kind` field (`easy_connect` | `tns_alias`). Files without `kind` load as `easy_connect` (`#[serde(default)]`), so existing profiles keep working.
- Planned, none in the spec: `logs/` and optional `scripts/` autosave (beyond spec). SQL history and settings sync are out of scope in the spec, so there is no `history.jsonl`.
- In memory: one `Catalog` per session; one Result Set window per executed statement.

## 6. Key decisions & trade-offs

| Decision | Solves | Worsens | Change it when |
|---|---|---|---|
| ODPI-C `oracle` driver | 11g support | every user needs Instant Client | 11g support is dropped (thin driver) |
| One worker thread per session | no concurrent use of one session; clean cancel | one thread + channel per session | > ~8 sessions → shared pool |
| Session per connection tab, new editors default to the active session (beyond spec) | Toad-like model; independent transactions | user must understand "which transaction am I in" | users want a shared session across tabs |
| TNS alias resolved by Oracle client | no custom parser in the hot path | alias errors surface as ORA-12154 | need to show resolved host/port |
| Paged fetch with row cap | bounded memory | "fetch more" UX | users need > 100k rows in the grid |
| ROWID-keyed edits applied on explicit commit (spec) | works without a primary key; matches the spec | rows can move or vanish; 0 rows updated must show a conflict | primary keys become common. Optional extension: also compare original values (not in spec) |
| Modular monolith (2 crates) | easy tests, no network | one deployable | a CLI/plugin needs the core |
| Windows-only | DPAPI only; simpler installer | no Linux/macOS | cross-platform becomes a goal |

Windows-only consequence: the base64 password fallback in `ProfileStore` stays only for non-Windows CI tests and must be marked unsupported.

## 7. Failure modes & degradation

| Failure | User sees | Degradation |
|---|---|---|
| Instant Client missing / wrong bitness (DPI-1047/1072) | remediation text at startup | editor still works offline |
| TNS alias not found (ORA-12154) | message naming the resolved `TNS_ADMIN` dir and known aliases | switch to Easy Connect |
| No listener / unknown service (ORA-12541/12514) | actionable message | profile stays saved |
| Session lost (ORA-03113/03114) | session marked Lost; buffers kept | never replay DML; reconnect on user action; warn that the transaction is gone |
| One session fails | only that tab shows an error | other sessions unaffected (bulkhead) |
| Runaway query | Cancel button | `break_execution`; bounded channel = backpressure |
| No call timeout on old clients | none | `set_call_timeout` exists in the crate but needs a newer client (likely 18.1+, unverified); Cancel is the only brake on 11.2 |
| Crash / close with open transaction | prompt on close | warn; optionally restore drafts from autosave (beyond spec) |

## 8. Scale evolution

- **Now:** paged fetch shipped with the Result Grid (issue #6, ADR-0002): 500-row pages, 10,000-row cap.
- **10×:** 100k-object catalogs → load columns lazily per table, cache with TTL, invalidate on DDL.
- **Signals to watch:** result memory, frame time, catalog load time, queued statements per session.

## 9. Spec alignment & follow-ups

**Resolved by the spec**
- Compile errors are in v1: stories 15–16 and ticket #9 (`ALL_ERRORS` with line and position).
- Windows-only, ROWID-keyed Data Grid and `DataGridMutator` as a pure tested seam all match the spec.

**Beyond the spec — each needs a new issue or a spec update**
| Item | Note |
|---|---|
| TNS aliases | spec profile = host, port, service/SID, credentials |
| Multiple concurrent sessions | spec story 3 is multiple *editor tabs* only |
| `DBMS_OUTPUT` capture | in neither the spec nor any ticket; decision pending |
| Script autosave | optional |
| Seam additions (cancel handle, paged fetch) | see the seam note in §3 |

**Gaps in the tickets**
- Story 3 (multiple editor tabs) has no open ticket.
- Ticket #6 has an extra criterion: odd and even rows use slightly different colors.
- The spec scopes the Schema Browser to the connected user via `ALL_*` views, so the code's `USER_OBJECTS` gives the same tree. Completion over other schemas or synonyms would need `ALL_OBJECTS` with an owner column; decide that in #11.

**Ticket → design piece**
| Ticket | Design piece |
|---|---|
| #6 (04) Execute → Result Grid | one `Session` entity lifted out of `ConnectionPanel`: worker thread, Command/Event, Cancel, paged fetch with row count, zebra rows |
| #7 (05) Row counts, commit/rollback, CSV | commit/rollback via `execute`, dirty flag, CSV from the fetched rows — shipped per ADR-0003 |
| #8 (06) Schema Browser | Catalog loaded lazily on expand; columns on table select |
| #9 (07) DDL viewer + recompile | `fetch_source` / `fetch_ddl`; `ALL_ERRORS` mapped through `Statement.start_line` |
| #10 (08) Editable Data Grid | `DataGridMutator`; join/view grids read-only |
| #11 (09) Completion | Catalog Cache; background load, never blocks typing |
| New issues | multiple editor tabs, TNS aliases, multi-session, `DBMS_OUTPUT` |

**Docs to update:** `GLOSSARY.md` (split *Connection* into Profile and Session; add TNS Alias if accepted); ADRs for session-per-worker and TNS (if accepted).

**Check before relying on it:** `oracle::Connection` must be shareable across threads so `break_execution` can run from the UI thread.

---
### Validation
- [x] Every trade-off row has a non-empty "Worsens" and a change condition.
- [x] Estimates carry units and assumptions.
- [x] Each critical dependency has a degradation path.
- [x] Every component ties to a requirement or number.
- [x] Requirements checked against issue #1 and tickets #6–#11; beyond-spec items are marked.
