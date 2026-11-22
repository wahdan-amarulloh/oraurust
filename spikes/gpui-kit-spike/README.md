# gpui-kit-spike

Throwaway spike validating [gpui-kit](https://crates.io/crates/gpui-kit) on Windows (issue #2): a dockable workspace with a PL/SQL-highlighted code editor and a 10,000-row virtualized table. Not shipped code.

## Binaries

- `cargo run --bin highlight-check` — headless smoke test, no window. Loads the vendored PL/SQL grammar, parses a sample package body, compiles the highlights query against it, runs the query, and asserts that `keyword`, `string`, `comment`, `number`, and `type` captures are present.
- `cargo run --bin spike` — the GUI spike. A dock area with a "PL/SQL Editor" panel (tree-sitter highlighting, line numbers, indent guides) in the center and a "Result Grid (10k)" panel — a 10,000-row synthetic employee table — docked at the bottom.

## Build prerequisites

- Rust (edition 2024; a recent stable toolchain)
- A C compiler for the build script (`cc` crate), e.g. MSVC on Windows

Build: `cargo build`. Run: the commands above.

## Vendored grammar

The PL/SQL grammar is [`njank/tree-sitter-plsql`](https://github.com/njank/tree-sitter-plsql) (MIT), vendored under `grammar/plsql/` (`parser.c`, `scanner.c`, C headers) and compiled by `build.rs` against tree-sitter 0.26.13. It is vendored because:

- The grammar's own Rust binding pins `tree-sitter ~0.25.9` and its build script has the external scanner commented out, so it cannot be used as a normal dependency.
- Vendoring lets us compile the parser verbatim against the runtime the editor uses (the ABI 15 parser is accepted by tree-sitter 0.26.13).

Highlighting queries in `grammar/highlights.scm` come from the same author's `zed-oracle-ext`; they use only capture names gpui-component's registry understands, so no remapping is needed. The query is checked at startup by compiling it against the grammar (a broken query would otherwise fail silently).

## Project structure

```
Cargo.toml              crate manifest; two [[bin]] targets
build.rs                compiles the vendored grammar C sources via cc
grammar/plsql/          vendored njank/tree-sitter-plsql parser + scanner
grammar/highlights.scm  highlights query (from zed-oracle-ext, adapted)
grammar/brackets.scm    brackets query (vendored alongside; not currently wired in)
src/lib.rs              crate root; exposes the plsql module
src/plsql.rs            grammar + query wiring into gpui-kit's LanguageRegistry
src/bin/highlight_check.rs  headless smoke test
src/bin/spike.rs        the dock + editor + table window
```

## Status

Spike only — expect it to be deleted or rewritten once issue #2 is resolved. Terms like *Script*, *Statement*, and *Result Grid* follow the repo root [`GLOSSARY.md`](../../GLOSSARY.md).
