# oraurust-core crate

The UI-free logic core (issue #4). Part of the cargo workspace rooted at the repo root.

## What it is

The logic behind the app, proven entirely by tests with no UI and no database:

- **ScriptSplitter** (`src/script.rs`): pure function, script text → statements,
  splitting on `;` and line-anchored `/` while respecting string literals
  (including `''` escapes), `--` comments and `/* */` comments. PL/SQL blocks —
  anonymous `begin`/`declare` and stored-unit definitions (`create [or replace]
  procedure/function/package/trigger/type ...`) — keep their internal
  semicolons; only a line-anchored `/` terminates them. Each `Statement` keeps
  its 1-based start/end lines and byte offsets for error-position mapping.
- **Statement under cursor** (`script::statement_at`, issue #6): pure function,
  script text + cursor byte offset → the statement to execute for Ctrl+Enter.
  A cursor between statements picks the nearest; a tie goes to the earlier one.
- **Connection seam** (`src/connection/`): the trait boundary between app and
  database — `execute`, `describe_schema`, `fetch_ddl`, `fetch_source` — with a
  scripted `MockConnection` returning canned outcomes (rows, row counts,
  transaction ends, `ORA-` errors). Cancel is a separate `CancelHandle`, not a
  fifth method (ADR-0002). `execute` takes `&self` so a statement runs on a
  background thread while the UI keeps the cancel handle to break it.
  `DbError::new` strips a code the Oracle message already repeats
  ("ORA-04043: object ..."), and `DbError` implements `Display`
  (`ORA-01756: message (at position N)`), so Oracle errors surface readably.
- **Connection profiles** (`src/connection/profile.rs`, `store.rs`, issue #5):
  `Profile` (name, host, port, service/SID, credentials) with Easy Connect
  string building (`//host:port/service`), and `ProfileStore`, a JSON file
  store whose passwords are encrypted at rest — DPAPI (`CryptProtectData`,
  current-user scope) on Windows, a clearly-marked base64 fallback for
  non-Windows test hosts.
- **Instant Client check** (`src/instant_client.rs`, issue #5): a probe for
  the Oracle Instant Client library (`oci.dll`) plus remediation text, and a
  classifier that maps ODPI-C's missing/incompatible-client errors (DPI-1047,
  DPI-1072) to an actionable message instead of a panic.
- **Transaction state** (`src/transaction.rs`, issue #7): the one place the
  clean/dirty rule lives. `TransactionEffect::of_outcome` maps an execution
  outcome to `Unchanged` (SELECT), `Dirty` (DML, a PL/SQL block) or `Clean`
  (DDL, COMMIT, ROLLBACK); `TransactionState::apply` folds it in. COMMIT and
  ROLLBACK are statements (`TransactionEnd`), executed through `execute`.
- **CSV export** (`src/csv.rs`, issue #7): pure `to_csv(&ResultSet) -> String`
  in RFC 4180 form — CRLF records, quoted/escaped fields, NULL as an empty
  field, an empty string as `""`.

Per ADR-0001, the real `Connection` is the `oracle` crate (ODPI-C) on a
background thread; that implementation lives in the app crate
(`crates/oraurust/src/oracle_conn.rs`) because it links the Instant Client.

## Layout

```
src/script.rs                ScriptSplitter + statement_at (pure)
src/connection/mod.rs        the Connection trait + shared types (ResultSet, DbError, ...)
src/connection/mock.rs       scripted MockConnection for tests
src/connection/profile.rs    Profile + Easy Connect string
src/connection/store.rs      ProfileStore (encrypted-at-rest persistence)
src/instant_client.rs        Instant Client probe + error remediation
src/transaction.rs           Transaction state and the clean/dirty rule
src/csv.rs                   Result Set → CSV (RFC 4180)
tests/script_splitter.rs     table-driven splitter tests (the seam's spec)
tests/statement_at.rs        statement-under-cursor tests
tests/connection_mock.rs     seam behavior through the mock
tests/db_error.rs            readable ORA- error text
tests/profile.rs             connect-string tests
tests/profile_store.rs       persistence + encryption-at-rest tests
tests/instant_client.rs      probe + error classification tests
tests/transaction.rs         the clean/dirty rule
tests/csv_export.rs          CSV format, NULL vs empty string, quoting
```

## Test

From the repo root: `cargo test -p oraurust-core`.

Domain vocabulary follows the repo root [`GLOSSARY.md`](../../GLOSSARY.md).
