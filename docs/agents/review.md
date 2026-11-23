# Review agents

Reviewing a diff is cheap on exploration and expensive on re-derivation. This
page lists what the repo has already settled, so a review does not spend its
budget re-reading sources that already answer the question.

## Do not re-derive these

- **Driver thread-safety.** `oracle::Connection` is `Send + Sync`; `Stmt` is
  `Send`. Cited with file:line in `docs/adr/0002-session-worker-cancel-paged-fetch.md`.
  Do not re-grep the vendored crate to confirm it.
- **Seam shape.** `Connection` has exactly four methods; cancel is a separate
  `CancelHandle`. Rationale and the paging contract are in ADR-0002. A diff that
  adds a fifth method to `Connection` is a finding; a diff that reads driver
  facts from the ADR is not.
- **Driver source map.** `CLAUDE.md` lists which `oracle-0.6.3` file answers
  which question. Use it instead of grepping the whole crate.

## What is mechanical vs judgement

Mechanical slips (formatting, lint, a broken test) are caught by
`scripts/pre-commit` and CI (`.github/workflows/ci.yml`), not by the review. If
the diff is already green there, spend review effort on judgement calls.

Spend review effort where the guardrail cannot reach:

- **Correctness under the seam.** Paging arithmetic (lost/duplicated rows, the
  peek-past-limit stash), cancel semantics, error mapping, the worker thread's
  ownership of the cursor.
- **UI-thread work.** Anything O(n) per page or per row that runs on the GPUI
  thread (copying the whole accumulated Result Set on each fetch was exactly
  this bug).
- **Spec fidelity.** Check the originating issue's acceptance criteria against
  the final state, not only the diff lines.
- **Documented contracts.** Does the code do what ADR-0002 and the `Connection`
  doc comments promise? A contradictory reply (e.g. `Rows { cursor: None }`
  while `more` is set) is a finding.

## Findings format

Two axes, reported separately, never reranked against each other:

- **Standards** — repo-documented rules (cite the file + rule) and Fowler smells
  (always judgement calls).
- **Spec** — missing/partial criteria, scope creep, implemented-but-wrong.

Keep each under ~400 words. Cite `file:line`.
