# oraurust app crate

The application crate (issues #3, #5, #6). Part of the cargo workspace rooted at the repo root.

## What it is

A native GPUI window (via `gpui-kit` 0.10) with a dock layout:

- **Center**: PL/SQL editor pane — tree-sitter highlighting, line numbers, indent guides, 4-space tabs. **Ctrl+Enter** runs the Statement under the cursor (issue #6).
- **Left dock**: placeholder for the future Schema Browser (issue #8).
- **Right dock**: Connection profiles (issue #5).
- **Bottom dock**: the Result Grid (issue #6) — a virtualized table with column names and types, the fetched row count, and a Cancel button for a running statement.

All four areas are resizable via the dock's drag handles; the docks are collapsible.

## Execution model (issue #6)

Ctrl+Enter splits the editor buffer with the pure `statement_at` and hands the statement's text to `src/runner.rs`. `QueryRunner` executes on GPUI's background executor, so the UI never blocks on Oracle; Cancel calls `Connection::cancel` (`break_execution`), also off the UI thread. The Result Grid renders from the runner's status: a SELECT becomes a striped (`DataTable::stripe`) virtualized grid whose headers read `NAME : TYPE`, DML shows the affected row count (still uncommitted), DDL reports an implicit commit, and an Oracle error shows its `ORA-` code, message and position via `DbError`'s `Display`.

## Layout

```
build.rs            compiles the vendored grammar C sources via cc
grammar/plsql/      vendored njank/tree-sitter-plsql parser + scanner (MIT)
grammar/highlights.scm  highlights query (from zed-oracle-ext)
src/main.rs         app entry: window creation, Quit action, startup query check
src/workspace.rs    the dock workspace, editor panel, Ctrl+Enter wiring
src/plsql.rs        grammar + query wiring into gpui-kit's LanguageRegistry
src/connection_panel.rs  Connection profiles panel (issue #5)
src/session.rs      the live session shared by the panels (issue #6)
src/runner.rs       QueryRunner: background execution, cancel, run status
src/result_grid.rs  the Result Grid panel (issue #6)
src/oracle_conn.rs  the real Connection: oracle crate over Instant Client
```

## Run and test

From the repo root (workspace root): `cargo run` to launch the app, `cargo build` to build without running. The workspace has one binary, `oraurust`; the run command opens the native window. Closing the window exits the process cleanly.

`cargo test` (also from the repo root) runs the logic core's tests from
`crates/core`: the `ScriptSplitter`, `statement_at`, and the `Connection` seam
with its mock (issues #4, #6). The app crate itself has no unit tests — its UI
views are not unit tested; the tested seams live in `crates/core`.

The app crate has no UI tests. Its one database test is the opt-in module at the
bottom of `src/oracle_conn.rs`: it drives the real `OraConnection` against a live
11g database, because the driver is invisible to the mocked core seams (the
0-based column indexing bug of issue #6 is exactly what a mock hides). It is
skipped unless `ORAURUST_IT=1` and all of `ORAURUST_IT_USER`,
`ORAURUST_IT_PASSWORD`, `ORAURUST_IT_CONNECT_STRING` (e.g. `//host:1521/service`)
are set. Run it with `cargo test -p oraurust oracle_conn`.

## Vendored grammar

The PL/SQL grammar is [`njank/tree-sitter-plsql`](https://github.com/njank/tree-sitter-plsql) (MIT), vendored because its own Rust binding pins tree-sitter ~0.25.9 and its build script has the external scanner commented out. We compile `parser.c`/`scanner.c` verbatim against tree-sitter 0.26.13 (ABI 15 parser accepted) — the pattern validated by the issue #2 spike. The highlights query is compiled at startup; a broken query would otherwise fail silently as a grey editor.

Gotcha from the spike: this grammar rejects underscored numeric literals (`30_000`); use `30000`.

Domain vocabulary follows the repo root [`GLOSSARY.md`](../../GLOSSARY.md).
