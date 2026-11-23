# ADR-0002: Session worker thread, separate cancel handle, paged SELECT

## Status

Accepted (issue #6)

## Context

The spec (issue #1) fixes `Connection` at four methods — `execute`,
`describe_schema`, `fetch_ddl`, `fetch_source` — and names it a tested
boundary. Issue #6 needs three things that four blocking methods cannot
express:

1. **Execution off the UI thread, with a cancel control.** `execute` blocks
   for the whole statement, so cancel cannot go through it: a second call
   would queue behind the blocked one and never run.
2. **A SELECT fetched in bounded pages** (`fetch_array_size` ≈ 500, a row
   cap), not materialised entirely in memory.
3. **The grid showing how many rows have been fetched so far**, and a way to
   fetch more.

The design doc (`docs/design/high-level-design.md` §3, §8) asks for these
changes to be recorded in an ADR, and for a check that `oracle::Connection`
can be shared across threads before cancel is relied on from the UI side.

## Decision

**`Connection` stays at four methods.** `execute(&self, &str) -> Result<StatementOutcome>`
is unchanged in shape; `StatementOutcome::Rows` now carries a first `Page`
plus an optional `Cursor` instead of a whole `ResultSet`.

**Cancel is a separate trait, `CancelHandle`** (`cancel(&self) -> Result<()>`),
not a fifth `Connection` method. It is obtained from the concrete driver
(`OraConnection::cancel_handle`) and called directly, bypassing the worker's
command queue — the only way to interrupt a blocked `execute`.

**A `Session` worker thread owns the connection and the open cursor**
(`crates/oraurust/src/session.rs`). The UI talks to it through a bounded
`flume` channel of `Command`s (`Execute`, `Fetch`) and replies. The worker
serialises OCI calls; the cursor stays on the worker so its server-side
resources live exactly as long as the Result Set.

**A SELECT is paged.** `Connection::execute` fetches one page
(`FETCH_ARRAY_SIZE = 500`); the app then keeps fetching pages through a
`Cursor` until the Result Set ends or `ROW_CAP = 10_000` is reached, at which
point `Page.capped` is set. `Page.fetched` is cumulative so the grid shows
"N rows fetched"; `Page.more` says whether a further page exists.

### Thread-sharing check

`oracle` 0.6.3 proves the session is shareable:

- `connection.rs:588-589` — `impl AssertSync for Connection {}` and
  `impl AssertSend for Connection {}` (assertion traits at `lib.rs:204-206`).
- `statement.rs:492` — `impl AssertSend for Stmt {}`.

So `Arc<oracle::Connection>` may cross to the worker thread and the cancel
handle may call `break_execution` from another thread while `execute` runs on
the worker.

## Consequences

- `MockConnection` scripts pages (`with_pages_for`) and a `MockCancel`, so the
  seam stays testable without a database.
- Cancel is best-effort: `break_execution` raises `ORA-01013` on the
  interrupted call, which surfaces as the run's error and is labelled
  "Cancelled" in the grid. The session stays usable afterwards.
- `Page.more` is computed exactly by reading one row past each page's limit
  and stashing it for the next page, so a peek never loses a row.
- A new statement abandons the previous cursor; its server-side resources free
  when the worker drops it.
- Fetching past the cap is refused in the worker; the grid keeps the rows up
  to the cap and the status line says so.
- If a future ticket needs to stream past the cap, it should bypass `ROW_CAP`
  explicitly rather than raise it for every SELECT. (CSV export, #7, shipped
  as a pure function over the fetched rows instead — see ADR-0003.)
