# Glossary

## Script
A buffer of SQL/PL/SQL text. Statements are separated by `;` (SQL) or a line-anchored `/` (PL/SQL block terminator).

## Statement
A single executable unit extracted from a Script: either a SQL statement (SELECT/DML/DDL) or an anonymous PL/SQL block.

## Execution
Running one Statement against a Connection. A SELECT produces a Result Set; DML produces a row count within the session's open Transaction; DDL commits implicitly (Oracle semantics).

## Transaction
The Oracle session's implicit transaction. DML stays uncommitted until the user commits or rolls back. The app never auto-commits DML.

## Result Set
Tabular output of a SELECT: column metadata (name, type) plus rows, displayed in a Result Grid.

## Result Grid
Virtualized read-only table view of a Result Set, exportable to CSV.

## Data Grid
An editable Result Grid over a single-table SELECT: cell edits generate an `UPDATE ... WHERE ROWID = :rid`, applied only on explicit commit.

## Schema Browser
Tree of the connected user's visible objects from the data dictionary (`ALL_OBJECTS` et al.), grouped by object type.

## Object
A dictionary entity: table, view, package (spec/body), procedure, function, trigger, sequence, synonym, index.

## DDL Viewer
Panel showing an Object's DDL via `DBMS_METADATA.GET_DDL`, or its source via `ALL_SOURCE` for program units.

## Catalog
The in-memory cache of Schema Browser metadata used for schema-aware completion.

## Completion
Identifier suggestions (tables, columns, keywords) built from the Catalog — there is no external PL/SQL language server; this is built in-app.

## Connection
A named, saved **Profile** (host, port, service/SID, credentials). Requires Oracle Instant Client on the machine. The live side it opens is the **Session** (below).

## Session
The live connection to the database, opened from a Profile. `Session` owns one worker thread that serialises OCI calls and holds any open Result Set cursor; the UI never calls the driver directly (ADR-0002).
