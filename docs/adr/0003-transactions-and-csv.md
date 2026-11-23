# ADR-0003: Transactions via `execute`, dirty state derived from outcomes

## Status

Accepted (issue #7)

## Context

Issue #7 needs three things around Oracle's implicit session transaction:

1. **DML row counts** surfaced, with nothing auto-committing.
2. **Explicit Commit/Rollback** from the UI, with the transaction's state
   visible.
3. **CSV export** of any Result Grid.

The seam note in `docs/design/high-level-design.md` §3 pre-decides the first
two: commit and rollback go through `execute("COMMIT")` / `execute("ROLLBACK")`,
so `Connection` stays at four methods (ADR-0002), and the dirty flag is derived
from outcomes.

The driver defaults to autocommit **off**: `oracle-0.6.3/src/connection.rs:543`
(`autocommit: AtomicBool::new(false)`), and `statement.rs:820` only adds
`DPI_MODE_EXEC_COMMIT_ON_SUCCESS` when it is on. `Connection::set_autocommit`
exists (`connection.rs:972`).

## Decision

**COMMIT/ROLLBACK are statements.** `StatementOutcome` gains a fourth shape,
`Transaction(TransactionEnd)` (`Commit` | `Rollback`). `OraConnection::execute`
classifies them by `stmt.statement_type()` **before** the DML path, because a
COMMIT reports no row count and would otherwise look like a DML statement with
zero rows.

**The dirty rule lives in core, once.** `TransactionEffect::of_outcome` maps an
outcome to `Unchanged` (SELECT), `Dirty` (DML row count, including a PL/SQL
block), or `Clean` (DDL, COMMIT, ROLLBACK). `TransactionState::apply` folds it
into `Clean` | `Dirty`. The worker thread sends the effect alongside the
outcome, and the app only applies it — no second copy of the rule.

**A row count means DML; everything else that is not a query, a COMMIT or a
ROLLBACK is DDL-shaped.** The driver adapter only takes `row_count()` for
`is_dml()`/`is_plsql()` statements. rust-oracle's `is_ddl()` covers only CREATE/
ALTER/DROP (`statement.rs:1161`), while Oracle also commits implicitly after
TRUNCATE, GRANT, REVOKE, COMMENT and ANALYZE — those report no affected rows and
would otherwise be misread as "0 rows affected (uncommitted)".

**A failed Commit/Rollback leaves the state alone.** The app clears the dirty
flag only when the server confirms; a failure is shown beside the Transaction
indicator ("Transaction still open: …") and the transaction stays open. The
status line reports the end the worker actually reported, not the one requested.

**Autocommit is set off explicitly** at connect, rather than inherited from the
driver default, so the Transaction UI's premise cannot silently change.

**The Transaction belongs to the session, not to a statement.** Commit/Rollback
stay enabled while a SELECT's grid is showing if DML is pending; a new
statement does not reset the flag, and a disconnect does.

**CSV export** is a pure function in core, `csv::to_csv(&ResultSet) -> String`
(RFC 4180: CRLF records, quoted/escaped fields, NULL as an empty field and an
empty string as `""`). The app formats it on the UI thread — bounded by
`ROW_CAP` — and writes the file on the background executor after the platform
save dialog (`App::prompt_for_new_path`).

## Consequences

- `MockConnection` scripts transaction ends with `with_transaction_end`, and
  core's `TransactionState` is tested without a database.
- Export covers what the grid holds: at most `ROW_CAP` rows. Streaming past the
  cap would need a cursor-aware exporter, which ADR-0002 says should bypass
  `ROW_CAP` explicitly rather than raise it for every SELECT.
- A PL/SQL block that commits internally leaves a stale "uncommitted" flag. The
  opposite (treating blocks as clean) would hide real pending DML, so the safe
  direction is chosen; issue #13 (`DBMS_OUTPUT`) may revisit this.
