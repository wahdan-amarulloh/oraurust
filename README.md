# oraurust

Work toward an Oracle SQL/PL/SQL client built in Rust (see GitHub issue #1 for the spec). Domain vocabulary is defined in [`GLOSSARY.md`](GLOSSARY.md); architecture decisions in `docs/adr/`.

## Contents

- `crates/oraurust/` — the application (cargo workspace root at the repo root). Currently the app shell (issue #3), Connection profiles (issue #5), and Ctrl+Enter execution into a Result Grid (issue #6): a native GPUI window with a dock layout — PL/SQL editor pane in the center (vendored `njank/tree-sitter-plsql` grammar), Connection panel on the right, Schema Browser placeholder on the left, and the Result Grid at the bottom. A `Session` owns a worker thread that serialises OCI calls; a SELECT is fetched in 500-row pages up to a 10,000-row cap, and a Cancel control breaks the running statement (ADR-0002). Run with `cargo run` from the repo root.
- `spikes/gpui-kit-spike/` — throwaway spike validating gpui-kit on Windows: a dockable workspace with a PL/SQL-highlighted editor and a 10,000-row virtualized table. Excluded from the workspace; see its [README](spikes/gpui-kit-spike/README.md).
- `docs/` — project docs, including `docs/adr/` and agent guides under `docs/agents/`.
- `docs/design/high-level-design.md` — high-level design (session/worker model, seams, failure handling)
- `scratch/` — scratch space; not part of the project.

## Build prerequisites

- Rust (edition 2024; a recent stable toolchain)
- A C compiler for the grammar build script (`cc` crate), e.g. MSVC on Windows
- gpui-kit 0.10 is pinned in the root `Cargo.lock`; commit that file for reproducible builds from a clean clone

## Run and test

All commands are run from the repo root, where the workspace lives. The workspace has a single binary, `oraurust`.

- `cargo run` — builds and launches the app. This opens the native window.
- `cargo build` — builds the workspace without running anything.
- `cargo test` — runs the workspace tests. The logic core `crates/core/` (issues #4, #6) carries the tested seams `ScriptSplitter`, `statement_at` (statement under the cursor), and `Connection` (mocked, with paged SELECTs and a separate `CancelHandle`); `cargo test -p oraurust-core` runs them alone. `DataGridMutator` arrives in a later ticket.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` — the same checks CI runs. Install them as a pre-commit hook with `cp scripts/pre-commit .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit`.

The optional integration tests against a real Oracle database are covered in [`docs/testing/integration-tests.md`](docs/testing/integration-tests.md).

Closing the window exits the process cleanly.

The `spikes/gpui-kit-spike/` crate is excluded from the workspace, so its own `cargo build` / `cargo run` must be invoked after `cd`-ing into that directory; see its [README](spikes/gpui-kit-spike/README.md).

