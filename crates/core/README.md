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
  its 1-based start/end lines for error-position mapping.
- **Connection seam** (`src/connection/`): the trait boundary between app and
  database — `execute`, `describe_schema`, `fetch_ddl`, `fetch_source` — with
  a scripted `MockConnection` returning canned outcomes (rows, row counts,
  `ORA-` errors).
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

Per ADR-0001, the real `Connection` is the `oracle` crate (ODPI-C) on a
background thread; that implementation lives in the app crate
(`crates/oraurust/src/oracle_conn.rs`) because it links the Instant Client.

## Layout

```
src/script.rs                ScriptSplitter (pure)
src/connection/mod.rs        the Connection trait + shared types (ResultSet, DbError, ...)
src/connection/mock.rs       scripted MockConnection for tests
src/connection/profile.rs    Profile + Easy Connect string
src/connection/store.rs      ProfileStore (encrypted-at-rest persistence)
src/instant_client.rs        Instant Client probe + error remediation
tests/script_splitter.rs     table-driven splitter tests (the seam's spec)
tests/connection_mock.rs     seam behavior through the mock
tests/profile.rs             connect-string tests
tests/profile_store.rs       persistence + encryption-at-rest tests
tests/instant_client.rs      probe + error classification tests
```

## Test

From the repo root: `cargo test -p oraurust-core`.

Domain vocabulary follows the repo root [`GLOSSARY.md`](../../GLOSSARY.md).
