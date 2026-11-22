# Spec: oraurust v0.1 — PL/SQL desktop editor for Oracle 11g

Status: ready-for-agent (local spec; publish to GitHub when remote exists)
Related: ADR-0001 (Oracle 11g requires ODPI-C driver)

## Problem Statement

A solo PL/SQL developer working daily against **Oracle 11g** has no fast, native, personal IDE. Toad is heavy and commercial; SQL Developer is slow and Java-based; the developer's own Zed extension (`zed-oracle-ext`) provides syntax highlighting but no execution, result grid, or schema browsing.

## Solution

A Windows desktop application built in Rust with GPUI (via `gpui-kit`) that provides: saved Oracle connections, a PL/SQL editor with tree-sitter syntax highlighting, statement execution with result and data grids, an object DDL viewer, a stored-procedure editor, a schema browser, and schema-aware completion built from the data dictionary.

## User Stories

1. As a PL/SQL developer, I want to save named connection profiles (host, port, service/SID, credentials), so that I can reconnect to my databases in one click.
2. As a developer, I want a clear startup error if Oracle Instant Client is missing, so that I know how to fix the setup.
3. As a developer, I want to open multiple SQL editor tabs, so that I can work on several scripts at once.
4. As a developer, I want PL/SQL syntax highlighting (packages, procedures, functions, triggers, anonymous blocks), so that I can read code quickly.
5. As a developer, I want bracket matching and auto-indentation for PL/SQL blocks, so that code stays readable.
6. As a developer, I want to run the statement under the cursor with Ctrl+Enter, so that execution matches familiar IDE muscle memory.
7. As a developer, I want a whole script with mixed SQL and PL/SQL to execute correctly, splitting on `;` and line-anchored `/`, so that I can run deployment scripts as-is.
8. As a developer, I want DML to report the affected row count and remain in an open transaction until I commit or roll back, so that I control transaction boundaries.
9. As a developer, I want to commit and roll back explicitly, so that I never lose or accidentally persist work.
10. As a developer, I want SELECT results in a virtualized grid with column names and types, so that large result sets never freeze the UI.
11. As a developer, I want the result grid to show the total row count fetched, so that I know the size of the result.
12. As a developer, I want to export a result grid to CSV, so that I can share data.
13. As a developer, I want a schema browser tree (tables, views, packages, procedures, functions, triggers, sequences, synonyms, indexes) from the data dictionary, so that I can explore the database without writing dictionary queries.
14. As a developer, I want to view an object's DDL (`DBMS_METADATA.GET_DDL`) or source (`ALL_SOURCE`) from the browser, so that I can read existing code.
15. As a developer, I want to open a package/procedure/function into an editable tab and re-compile it with Ctrl+Enter, so that I can maintain stored code in-app.
16. As a developer, I want compile errors surfaced with line and position (`ALL_ERRORS`), so that I can fix broken PL/SQL in place.
17. As a developer, I want schema-aware completion for table and column names from the live connection, so that I stop mistyping identifiers.
18. As a developer, I want to edit cells in a single-table data grid, generating an `UPDATE` keyed on `ROWID` and applied only on explicit commit, so that I can fix data without writing UPDATE statements.
19. As a developer, I want data grids over joins or views without a stable row identity to be read-only, so that I never corrupt data through a guessed key.
20. As a developer, I want long queries to run on a background thread with a cancel option, so that the UI never freezes.
21. As a developer, I want clear Oracle error messages (`ORA-` codes with position), so that I can diagnose failures fast.
22. As a developer, I want completion not to block typing while it loads the catalog, so that the editor stays responsive.

## Implementation Decisions

- **UI framework**: `gpui-kit` (Longbridge) — actively maintained, Windows-supported, ships a code editor with tree-sitter highlighting and completion hooks, a dock/split layout, and a virtualized table. Raw `gpui` from crates.io is stale (no release in 13 months) and tied to Zed's repo. Forking Zed is rejected: ~300 unpublished crates, GPL-3.0 editor, weekly upstream releases.
- **Grammar**: reuse `njank/tree-sitter-plsql` (MIT) — the same grammar the developer's `zed-oracle-ext` (MIT) pins — together with the extension's `highlights.scm`, bracket configuration, and indentation/injection queries where compatible.
- **Driver**: `oracle` 0.6.3 (ODPI-C) per ADR-0001. Oracle 11g floor forces Oracle Instant Client on the machine; `oracledb` (pure-Rust thin driver) is excluded because it supports 19c+ only. All database calls are synchronous and run on a dedicated background executor; the GPUI thread never blocks.
- **No external language server**: no production PL/SQL LSP exists (`sqls` is plain SQL; the VS Code PL/SQL tooling is proprietary). Completion is built in-app from the Catalog (`ALL_OBJECTS`, `ALL_TAB_COLUMNS`, related `ALL_*` views) and surfaced through the editor's completion API.
- **Seams** (the only boundaries across which logic is tested):
  - `Connection` — `execute(statement) -> StatementOutcome`, `describe_schema() -> Catalog`, `fetch_ddl(object) -> String`, `fetch_source(object) -> String`. All Oracle-specific code (driver, dictionary SQL, `DBMS_METADATA`) lives behind it.
  - `ScriptSplitter` — pure: script text → `Vec<Statement>`, splitting on `;` and line-anchored `/` while respecting string literals and comments.
  - `DataGridMutator` — pure: staged cell edits → parameterized `UPDATE ... WHERE ROWID = :rid` statements.
- **Data Grid edits**: single-table SELECTs are editable; edits stage in memory and apply as one transaction on commit. Grids over joins/views without a `ROWID` are read-only.
- **Result Set metadata**: column names and types taken from the statement's column metadata (describe-only), so the grid renders before/without fetching all rows.
- **Schema Browser**: queries `ALL_*` dictionary views scoped to the connected user, grouped by object type; lazily loads children on expand.

## Testing Decisions

- Good tests assert **external behavior** through the public boundary of a seam — e.g. "a script with three `;` statements and one `/` block yields four statements" or "an edited cell produces an UPDATE keyed on the original ROWID" — never internal helper calls.
- Modules tested:
  - `ScriptSplitter`: statement splitting across string literals, comments, `/` terminators, empty scripts, trailing whitespace.
  - `Connection`: mocked implementation for result metadata, DML row counts, DDL/source fetch, and Oracle error surfacing; optional integration tests against a real 11g database when `ORAURUST_IT=1`.
  - `DataGridMutator`: staging, editing a cell twice, cancelling, commit ordering, read-only grids rejecting edits.
- Prior art: the repo is empty, so these establish the prior art; use Rust's standard unit tests with `rstest`-style cases for the table-driven splitter tests.
- UI views are not unit-tested; seam-level tests cover the logic they call.

## Out of Scope

- Stored procedure debugger.
- Explain plan.
- SQL history.
- Schema compare.
- Session/trace monitoring.
- Multiple result sets per script/tab.
- Migration to Oracle's thin driver / 19c-only support.
- Non-Windows platforms.
- Installers, auto-update, multi-user settings sync.

## Further Notes

- This spec is stored at `.scratch/specs/` because the repository has no git remote yet. On first push: `gh issue create` with this body and apply `ready-for-agent`.
- Domain vocabulary is defined in `GLOSSARY.md`; the driver trade-off is recorded in `docs/adr/0001-oracle-11g-driver.md`.
</spec-template>
