# oraurust

Work toward an Oracle SQL/PL/SQL client built in Rust (see GitHub issue #1 for the spec). Domain vocabulary is defined in [`GLOSSARY.md`](GLOSSARY.md); architecture decisions in `docs/adr/`.

## Contents

- `spikes/gpui-kit-spike/` — throwaway spike validating gpui-kit on Windows: a dockable workspace with a PL/SQL-highlighted editor (vendored `njank/tree-sitter-plsql` grammar) and a 10,000-row virtualized table. See its [README](spikes/gpui-kit-spike/README.md).
- `docs/` — project docs, including `docs/adr/` and agent guides under `docs/agents/`.
- `scratch/` — scratch space; not part of the project.
