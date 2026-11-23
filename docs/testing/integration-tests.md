# Integration tests against a real Oracle database

The driver is invisible to the mocked core seams, so bugs that only ODPI-C can
show (a wrong column index, a wrong error mapping, paging arithmetic) slip past
`cargo test` and surface in the app. These tests close that gap. They are
**opt-in**: skipped unless `ORAURUST_IT=1` and the connection variables are set.

Run them whenever you touch `crates/oraurust/src/oracle_conn.rs`, the
`Connection`/`Cursor` seam, or anything that maps driver errors.

## What needs to be present

1. **Oracle Instant Client** (Basic or Basic Light, 64-bit) on `PATH` — see
   ADR-0001. The same check the app runs at startup applies here; a missing
   client fails the test with the actionable message, not a raw DPI- error.
2. **A reachable database.** Any 11g+ instance works. For a local one, the
   `gvenzl/oracle-free` image is the smallest disposable option:

   ```sh
   docker run -d --name oraurust-it -p 1521:1521 \
     -e ORACLE_PASSWORD=oracle gvenzl/oracle-free:23-slim
   # wait for "DATABASE IS READY TO USE!" in: docker logs -f oraurust-it
   ```

## Running

PowerShell:

```powershell
$env:ORAURUST_IT = "1"
$env:ORAURUST_IT_USER = "system"
$env:ORAURUST_IT_PASSWORD = "oracle"
$env:ORAURUST_IT_CONNECT_STRING = "//localhost:1521/FREEPDB1"
cargo test -p oraurust -- --nocapture
```

POSIX shell:

```sh
ORAURUST_IT=1 \
ORAURUST_IT_USER=system \
ORAURUST_IT_PASSWORD=oracle \
ORAURUST_IT_CONNECT_STRING='//localhost:1521/FREEPDB1' \
cargo test -p oraurust -- --nocapture
```

`--nocapture` shows the `skipped: ...` lines, so a mis-set variable is visible
rather than silently passing.

## What the suite covers

- Single- and multi-column SELECTs, including `NULL` staying `None`.
- `describe_schema` reading both dictionary columns.
- ORA- errors: code shown once, 1-based position preserved.
- DML affected row count, never committed.
- **Transactions**: COMMIT/ROLLBACK classified as `StatementOutcome::Transaction`
  (`commit_and_rollback_are_classified_as_transaction_ends`), TRUNCATE and other
  implicitly-committing statements reported as `Ddl` not `RowCount`
  (`statements_outside_the_transaction_are_reported_as_ddl`), and a rollback
  discarding an uncommitted insert into a session-private global temporary table
  (`rollback_discards_uncommitted_dml`). Every table the suite creates is dropped
  afterwards.
- **Paging**: 1200 rows over three 500-row pages, exact end-of-data, no lost or
  duplicated rows (`a_select_larger_than_one_page_needs_the_cursor`).

The suite never commits DML and uses `dual` / `CONNECT BY LEVEL` where possible,
so it is safe against a shared development database.

## Not covered yet

Cancel (`break_execution`) and "the session stays usable afterwards" are proven
only statically (ADR-0002: `oracle::Connection` is `Send + Sync`). A runtime
cancel test would need a long-running statement kept on one thread while the
cancel handle fires from another. Add it here when the app's Cancel path changes.
