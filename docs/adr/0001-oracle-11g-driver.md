# ADR-0001: Oracle 11g requires the ODPI-C driver

## Status

Accepted

## Context

The tool must connect to Oracle **11g** databases. Two Rust Oracle drivers exist:

- `oracledb` (Oracle's official pure-Rust thin driver, 26.0 beta): zero installation, but supports **19c+ only**.
- `oracle` 0.6.3 (ODPI-C wrapper): supports 11g through 23ai, but requires **Oracle Instant Client** (11.2+ DLLs) installed on every Windows machine, and is a blocking (sync) API.

## Decision

Use the `oracle` crate 0.6.3 (ODPI-C). Ship a startup check that detects a missing/incompatible Instant Client and shows an actionable message with a download link. Run all database work on a dedicated background thread; the GPUI thread never blocks on Oracle.

## Consequences

- Every user must install Oracle Instant Client (Basic or Basic Light) — a real onboarding cost that Toad also imposes.
- The API is synchronous; we must isolate it behind the `Connection` seam and dispatch via `cx.background_executor`.
- Metadata: `column_info()` and describe-only execution are available for Result Set column types without fetching all rows.
- If 11g support is ever dropped, the driver can be swapped behind the same seam for the zero-install thin driver.
