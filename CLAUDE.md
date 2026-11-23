# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Agent skills

### Issue tracker

Issues live in the repo's GitHub Issues, managed via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` at the repo root + `docs/adr/`. See `docs/agents/domain.md`.

## Driver source map

The `oracle` crate is vendored under `~/.cargo/registry/src/index.crates.io-*/oracle-0.6.3/src/`.
Read these before grepping the whole crate:

- `connection.rs` — `Connection`: `Send`/`Sync` assertions (~588), `break_execution` (~1009), `query`, `set_call_timeout`.
- `statement.rs` — fetch tuning (`fetch_array_size`, `prefetch_rows`), `Statement::into_result_set` (owns the statement, so a cursor can outlive it), the private `more_rows`.
- `row.rs` — `ResultSet` iterator, `column_info`, `Row::get` (0-based column index).

Established driver facts live in `docs/adr/0001-oracle-11g-driver.md` and
`docs/adr/0002-session-worker-cancel-paged-fetch.md` with file:line citations.
Check the ADRs before re-deriving anything about the driver.

## Review agents

Reviewing a diff needs no re-exploration of facts the repo already records. Read
`docs/agents/review.md` first; it lists what is already settled and where.
