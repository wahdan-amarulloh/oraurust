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

Per ADR-0001, the real `Connection` will be the `oracle` crate (ODPI-C) on a
background thread; that wiring arrives with ticket #5.

## Layout

```
src/script.rs           ScriptSplitter (pure)
src/connection/mod.rs   the Connection trait + shared types (ResultSet, DbError, ...)
src/connection/mock.rs  scripted MockConnection for tests
tests/script_splitter.rs    table-driven splitter tests (the seam's spec)
tests/connection_mock.rs    seam behavior through the mock
```

## Test

From the repo root: `cargo test -p oraurust-core`.

Domain vocabulary follows the repo root [`GLOSSARY.md`](../../GLOSSARY.md).
