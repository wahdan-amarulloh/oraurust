# oraurust app crate

The application crate (issue #3, app shell). Part of the cargo workspace rooted at the repo root.

## What it is

A native GPUI window (via `gpui-kit` 0.10) with a dock layout:

- **Center**: PL/SQL editor pane — tree-sitter highlighting, line numbers, indent guides, 4-space tabs.
- **Left dock**: placeholder for the future Schema Browser (issue #8).
- **Bottom dock**: placeholder for the future Result Grid (issue #6).

All three areas are resizable via the dock's drag handles; the docks are collapsible. No database functionality yet.

## Layout

```
build.rs            compiles the vendored grammar C sources via cc
grammar/plsql/      vendored njank/tree-sitter-plsql parser + scanner (MIT)
grammar/highlights.scm  highlights query (from zed-oracle-ext)
src/main.rs         app entry: window creation, Quit action, startup query check
src/workspace.rs    the dock workspace and its three panels
src/plsql.rs        grammar + query wiring into gpui-kit's LanguageRegistry
```

## Run and test

From the repo root (workspace root): `cargo run` to launch the app, `cargo build` to build without running. The workspace has one binary, `oraurust`; the run command opens the native window. Closing the window exits the process cleanly.

`cargo test` (also from the repo root) runs the logic core's tests from
`crates/core`: the `ScriptSplitter` and `Connection` seam with its mock
(issue #4). The app crate itself has no tests — its UI views are not unit
tested; the tested seams live in `crates/core`.

## Vendored grammar

The PL/SQL grammar is [`njank/tree-sitter-plsql`](https://github.com/njank/tree-sitter-plsql) (MIT), vendored because its own Rust binding pins tree-sitter ~0.25.9 and its build script has the external scanner commented out. We compile `parser.c`/`scanner.c` verbatim against tree-sitter 0.26.13 (ABI 15 parser accepted) — the pattern validated by the issue #2 spike. The highlights query is compiled at startup; a broken query would otherwise fail silently as a grey editor.

Gotcha from the spike: this grammar rejects underscored numeric literals (`30_000`); use `30000`.

Domain vocabulary follows the repo root [`GLOSSARY.md`](../../GLOSSARY.md).
