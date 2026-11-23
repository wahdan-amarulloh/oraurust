//! The real [`Connection`]: the `oracle` crate (ODPI-C) over Oracle Instant
//! Client, per ADR-0001. Built from a [`Profile`]'s Easy Connect string;
//! runs on the background thread (GPUI never blocks on Oracle).

use std::sync::Arc;

use oraurust_core::{
    CancelHandle, Catalog, Column, Connection, Cursor, DbError, FETCH_ARRAY_SIZE, FETCH_ROWS,
    Object, ObjectKind, Page, Profile, ROW_CAP, Result, StatementOutcome,
};

/// A live Oracle session behind the [`Connection`] seam.
///
/// The session is `Arc`-shared so [`OraConnection::cancel_handle`] can break a
/// running statement from another thread; `oracle::Connection` is `Send +
/// Sync` (ADR-0002).
pub struct OraConnection {
    session: Arc<oracle::Connection>,
}

impl OraConnection {
    /// Connects using `profile`'s Easy Connect string.
    ///
    /// Errors are mapped through `instant_client::classify_error`, so a
    /// missing/incompatible Instant Client surfaces as an actionable message
    /// rather than a raw DPI- error.
    pub fn connect(profile: &Profile) -> Result<Self> {
        Self::connect_with_connect_string(profile, profile.connect_string())
    }

    /// Connects with an explicit connect string instead of the one built from
    /// the profile — used by the opt-in integration test, which can point at a
    /// full descriptor the profile fields cannot express.
    fn connect_with_connect_string(
        profile: &Profile,
        connect_string: impl AsRef<str>,
    ) -> Result<Self> {
        oracle::Connection::connect(&profile.username, &profile.password, connect_string)
            .map(|session| Self {
                session: Arc::new(session),
            })
            .map_err(|e| map_error(e, None))
    }

    /// A handle that breaks the running statement (`break_execution`), over
    /// the same session (ADR-0002).
    pub fn cancel_handle(&self) -> Box<dyn CancelHandle> {
        Box::new(OraCancel {
            session: self.session.clone(),
        })
    }
}

/// [`CancelHandle`] for [`OraConnection`].
struct OraCancel {
    session: Arc<oracle::Connection>,
}

impl CancelHandle for OraCancel {
    fn cancel(&self) -> Result<()> {
        // `break_execution` is an OCI call: the caller runs it off the UI
        // thread. The server raises ORA-01013 on the interrupted fetch, which
        // surfaces as the run's error.
        self.session
            .break_execution()
            .map_err(|e| map_error(e, None))
    }
}

fn map_error(error: oracle::Error, position: Option<usize>) -> DbError {
    // ODPI-C errors about the client library itself (missing/incompatible
    // Instant Client) carry no db_error(), so the DPI code must be read off
    // the error in that branch too — otherwise classify_error never fires
    // and the user loses the remediation text.
    let Some(dberr) = error.db_error() else {
        let db_error = match error.dpi_code() {
            Some(dpi) => DbError::new(format!("DPI-{dpi}"), error.to_string(), None),
            None => DbError::new("", error.to_string(), None),
        };
        return map_classified(db_error);
    };
    let code = match error.oci_code() {
        Some(oci) => format!("ORA-{oci:05}"),
        None => error
            .dpi_code()
            .map(|dpi| format!("DPI-{dpi}"))
            .unwrap_or_default(),
    };
    // `offset()` is ODPI-C's 0-based parse-error offset (0 when it has none);
    // `DbError.position` is 1-based, so shift it and drop the "no position".
    let position = position.or_else(|| match dberr.offset() {
        0 => None,
        offset => Some(offset as usize + 1),
    });
    map_classified(DbError::new(code, dberr.message(), position))
}

/// Turns errors about the Instant Client itself into the actionable text the
/// startup check produces; leaves every other error alone.
fn map_classified(db_error: DbError) -> DbError {
    match oraurust_core::instant_client::classify_error(db_error.clone()) {
        Some(actionable) => actionable,
        None => db_error,
    }
}

impl Connection for OraConnection {
    fn execute(&self, statement: &str) -> Result<StatementOutcome> {
        let mut stmt = self
            .session
            .statement(statement)
            .fetch_array_size(FETCH_ARRAY_SIZE)
            // One extra row in the prefetch tells us whether more rows remain
            // without a round trip per page boundary.
            .prefetch_rows(FETCH_ARRAY_SIZE + 1)
            .build()
            .map_err(|e| map_error(e, None))?;
        // The statement type decides the outcome shape: SELECT → a first page
        // plus a cursor, DDL → implicit commit, DML → affected row count in
        // the open transaction.
        if stmt.is_query() {
            // `into_result_set` takes ownership of the statement, so the
            // cursor can outlive this call on the worker thread.
            let mut result = stmt
                .into_result_set::<oracle::Row>(&[])
                .map_err(|e| map_error(e, None))?;
            let columns = columns_of(&result);
            let mut pending = None;
            let (rows, more) = read_rows(&mut result, columns.len(), FETCH_ROWS, &mut pending)?;
            let fetched = rows.len();
            let cursor = more.then(|| {
                Box::new(OraCursor {
                    result: Some(result),
                    columns: columns.clone(),
                    fetched,
                    exhausted: false,
                    pending,
                }) as Box<dyn Cursor>
            });
            Ok(StatementOutcome::Rows {
                page: Page {
                    columns,
                    rows,
                    fetched,
                    capped: false,
                    more,
                },
                cursor,
            })
        } else {
            stmt.execute(&[]).map_err(|e| map_error(e, None))?;
            if stmt.is_ddl() {
                Ok(StatementOutcome::Ddl)
            } else {
                let affected = stmt.row_count().map_err(|e| map_error(e, None))?;
                Ok(StatementOutcome::RowCount(affected))
            }
        }
    }

    fn describe_schema(&self) -> Result<Catalog> {
        let mut objects: Vec<Object> = Vec::new();
        let sql = "SELECT object_name, object_type \
                   FROM user_objects \
                   WHERE object_type IN (\
                     'TABLE','VIEW','PACKAGE','PACKAGE BODY','PROCEDURE',\
                     'FUNCTION','TRIGGER','SEQUENCE','SYNONYM','INDEX') \
                   ORDER BY object_type, object_name";
        for row in self
            .session
            .query(sql, &[])
            .map_err(|e| map_error(e, None))?
        {
            let row = row.map_err(|e| map_error(e, None))?;
            // 0-based: column 0 is object_name, column 1 object_type.
            let name: String = row.get(0).map_err(|e| map_error(e, None))?;
            let kind_text: String = row.get(1).map_err(|e| map_error(e, None))?;
            if let Some(kind) = parse_object_kind(&kind_text) {
                objects.push(Object { name, kind });
            }
        }
        Ok(Catalog { objects })
    }

    fn fetch_ddl(&self, object: &Object) -> Result<String> {
        let sql = "SELECT DBMS_METADATA.GET_DDL(:1, :2) FROM dual";
        let kind = sql_name(object.kind).to_string();
        let name = object.name.clone();
        let ddl: String = self
            .session
            .query_row_as_named(sql, &[(":1", &kind), (":2", &name)])
            .map_err(|e| map_error(e, None))?;
        Ok(ddl)
    }

    fn fetch_source(&self, object: &Object) -> Result<String> {
        let sql = "SELECT text FROM all_source \
                   WHERE name = :1 AND type = :2 ORDER BY line";
        let kind = sql_name(object.kind).to_string();
        let name = object.name.clone();
        let rows: Vec<(String,)> = self
            .session
            .query_as_named(sql, &[(":1", &name), (":2", &kind)])
            .map_err(|e| map_error(e, None))?
            .collect::<oracle::Result<Vec<_>>>()
            .map_err(|e| map_error(e, None))?;
        let mut source = String::new();
        for (text,) in rows {
            source.push_str(&text);
        }
        if source.is_empty() {
            return Err(not_found(object));
        }
        Ok(source)
    }
}

/// Paged continuation of a SELECT (issue #6, ADR-0002).
///
/// Holds the `oracle` Result Set, which owns its statement, and the running
/// totals the grid shows. The row cap and page size come from core.
struct OraCursor {
    result: Option<oracle::ResultSet<'static, oracle::Row>>,
    columns: Vec<Column>,
    /// Rows fetched across every page so far.
    fetched: usize,
    /// Set once the Result Set has no more rows (or the cap was hit).
    exhausted: bool,
    /// A row read one past a page's limit, kept for the next page so the
    /// exact "more rows" check never loses a row.
    pending: Option<Vec<Option<String>>>,
}

impl Cursor for OraCursor {
    fn fetch(&mut self, max_rows: usize) -> Result<Page> {
        if self.exhausted {
            return Ok(self.page(Vec::new(), false));
        }
        let Some(result) = self.result.as_mut() else {
            return Ok(self.page(Vec::new(), false));
        };
        // Never fetch past the cap; the page stops at whichever comes first.
        let remaining = ROW_CAP.saturating_sub(self.fetched);
        let want = max_rows.min(FETCH_ROWS).min(remaining);
        let (rows, more) = read_rows(result, self.columns.len(), want, &mut self.pending)?;
        let capped = more && rows.len() + self.fetched >= ROW_CAP;
        self.fetched += rows.len();
        // End of data or the cap releases the server-side cursor.
        self.exhausted = !more || capped;
        if self.exhausted {
            self.result = None;
        }
        Ok(self.page(rows, capped))
    }
}

impl OraCursor {
    fn page(&self, rows: Vec<Vec<Option<String>>>, capped: bool) -> Page {
        Page {
            columns: self.columns.clone(),
            rows,
            fetched: self.fetched,
            capped,
            more: !self.exhausted,
        }
    }
}

/// The Result Set's column metadata, name and Oracle type (issue #6's grid
/// header).
fn columns_of(result: &oracle::ResultSet<'static, oracle::Row>) -> Vec<Column> {
    result
        .column_info()
        .iter()
        .map(|c| Column {
            name: c.name().to_string(),
            type_name: c.oracle_type().to_string(),
        })
        .collect()
}

/// Reads up to `limit` rows as strings and reports whether more remain.
///
/// Reads one row past `limit` to learn `more` exactly, stashing it in
/// `pending` so the next page starts with it — otherwise a peek would lose a
/// row. Every value is `Option<String>`; `NULL` stays `None`. Column indexes
/// are 0-based (rust-oracle's `Row::get`).
fn read_rows(
    result: &mut oracle::ResultSet<'static, oracle::Row>,
    columns: usize,
    limit: usize,
    pending: &mut Option<Vec<Option<String>>>,
) -> Result<(Vec<Vec<Option<String>>>, bool)> {
    let mut rows = Vec::with_capacity(limit.min(64));
    if let Some(row) = pending.take() {
        rows.push(row);
    }
    while rows.len() < limit {
        match result.next() {
            Some(row) => {
                let row = row.map_err(|e| map_error(e, None))?;
                rows.push(read_row(&row, columns)?);
            }
            None => return Ok((rows, false)),
        }
    }
    // Reached the limit: one more row decides `more`, and is kept for later.
    match result.next() {
        Some(row) => {
            let row = row.map_err(|e| map_error(e, None))?;
            *pending = Some(read_row(&row, columns)?);
            Ok((rows, true))
        }
        None => Ok((rows, false)),
    }
}

fn read_row(row: &oracle::Row, columns: usize) -> Result<Vec<Option<String>>> {
    (0..columns)
        .map(|ix| {
            row.get::<usize, Option<String>>(ix)
                .map_err(|e| map_error(e, None))
        })
        .collect()
}

fn not_found(object: &Object) -> DbError {
    DbError::new(
        "ORA-04043",
        format!("object {} does not exist", object.name),
        None,
    )
}

/// Dictionary label for each [`ObjectKind`] — the value `ALL_OBJECTS.object_type`,
/// `ALL_SOURCE.type` and `DBMS_METADATA.GET_DDL` all use. One table keeps the
/// two directions (parse ↔ emit) from drifting apart.
const OBJECT_KIND_LABELS: [(ObjectKind, &str); 10] = [
    (ObjectKind::Table, "TABLE"),
    (ObjectKind::View, "VIEW"),
    (ObjectKind::Package, "PACKAGE"),
    (ObjectKind::PackageBody, "PACKAGE BODY"),
    (ObjectKind::Procedure, "PROCEDURE"),
    (ObjectKind::Function, "FUNCTION"),
    (ObjectKind::Trigger, "TRIGGER"),
    (ObjectKind::Sequence, "SEQUENCE"),
    (ObjectKind::Synonym, "SYNONYM"),
    (ObjectKind::Index, "INDEX"),
];

fn parse_object_kind(kind_text: &str) -> Option<ObjectKind> {
    OBJECT_KIND_LABELS
        .iter()
        .find(|(_, label)| *label == kind_text)
        .map(|(kind, _)| *kind)
}

/// The dictionary label `DBMS_METADATA.GET_DDL` and `ALL_SOURCE.type` expect.
fn sql_name(kind: ObjectKind) -> &'static str {
    OBJECT_KIND_LABELS
        .iter()
        .find(|(known, _)| *known == kind)
        .map(|(_, label)| *label)
        .expect("every ObjectKind has a dictionary label")
}

/// Opt-in tests against a real database, because the driver is invisible to
/// the mocked core seams: a 1-based column index in `execute` made every
/// SELECT fail with "invalid column index 1 (zero-based)".
///
/// Skipped unless `ORAURUST_IT=1` and all of `ORAURUST_IT_USER`,
/// `ORAURUST_IT_PASSWORD`, `ORAURUST_IT_CONNECT_STRING` are set (see the
/// crate README).
#[cfg(test)]
mod integration {
    use super::*;

    fn connection() -> Option<OraConnection> {
        if std::env::var("ORAURUST_IT").ok().as_deref() != Some("1") {
            return None;
        }
        let profile = Profile {
            name: "it".into(),
            host: String::new(),
            port: 1521,
            service: String::new(),
            username: std::env::var("ORAURUST_IT_USER").ok()?,
            password: std::env::var("ORAURUST_IT_PASSWORD").ok()?,
        };
        let connect_string = std::env::var("ORAURUST_IT_CONNECT_STRING").ok()?;
        Some(
            OraConnection::connect_with_connect_string(&profile, connect_string)
                .expect("integration connection failed"),
        )
    }

    fn skip() {
        eprintln!("skipped: set ORAURUST_IT=1 and the ORAURUST_IT_* variables");
    }

    #[test]
    fn single_column_select_returns_its_one_value() {
        let Some(conn) = connection() else {
            return skip();
        };

        match conn
            .execute("select 1 from dual")
            .expect("select 1 from dual")
        {
            StatementOutcome::Rows { page, .. } => {
                assert_eq!(page.columns.len(), 1);
                assert_eq!(page.rows.len(), 1);
                assert_eq!(page.rows[0].len(), 1, "one value per column");
                assert_eq!(page.rows[0][0].as_deref(), Some("1"));
            }
            other => panic!("expected Rows, got {other:?}"),
        }
    }

    #[test]
    fn multi_column_select_returns_every_value_and_nulls() {
        let Some(conn) = connection() else {
            return skip();
        };

        let outcome = conn
            .execute("select 1 as a, 'x' as b, null as c from dual")
            .expect("three-column select");
        match outcome {
            StatementOutcome::Rows { page, .. } => {
                assert_eq!(page.columns.len(), 3);
                assert_eq!(page.rows.len(), 1);
                assert_eq!(page.rows[0].len(), 3);
                assert_eq!(page.rows[0][0].as_deref(), Some("1"));
                assert_eq!(page.rows[0][1].as_deref(), Some("x"));
                assert_eq!(page.rows[0][2], None, "NULL stays None");
            }
            other => panic!("expected Rows, got {other:?}"),
        }
    }

    #[test]
    fn describe_schema_reads_both_dictionary_columns() {
        let Some(conn) = connection() else {
            return skip();
        };

        let catalog = conn.describe_schema().expect("describe_schema");
        for object in &catalog.objects {
            assert!(!object.name.is_empty());
        }
    }

    #[test]
    fn oracle_errors_show_the_code_once_with_position() {
        let Some(conn) = connection() else {
            return skip();
        };

        let error = conn
            .execute("select no_such_column from dual")
            .expect_err("unknown column");
        assert_eq!(error.code, "ORA-00904");
        assert_eq!(
            error.to_string().matches("ORA-00904").count(),
            1,
            "the code must not be repeated: {error}"
        );
        assert!(
            error.position.is_some_and(|position| position >= 1),
            "Oracle reports a 1-based position: {error}"
        );
    }

    #[test]
    fn a_select_larger_than_one_page_needs_the_cursor() {
        let Some(conn) = connection() else {
            return skip();
        };

        // CONNECT BY LEVEL generates 1200 rows, past one 500-row page; no
        // table is touched and nothing is committed.
        let sql = "select level as n from dual connect by level <= 1200";
        let (first, mut cursor) = match conn.execute(sql).expect("paged select") {
            StatementOutcome::Rows { page, cursor } => (page, cursor.expect("more rows remain")),
            other => panic!("expected Rows, got {other:?}"),
        };
        assert_eq!(first.rows.len(), FETCH_ROWS);
        assert!(first.more);
        assert_eq!(first.fetched, FETCH_ROWS);

        let second = cursor.fetch(FETCH_ROWS).expect("second page");
        assert_eq!(second.rows.len(), FETCH_ROWS);
        assert_eq!(second.fetched, 2 * FETCH_ROWS);
        assert!(second.more, "1200 rows need a third page");

        let third = cursor.fetch(FETCH_ROWS).expect("third page");
        assert_eq!(third.rows.len(), 1200 - 2 * FETCH_ROWS);
        assert!(!third.more, "the Result Set ends at 1200 rows");
        assert_eq!(third.fetched, 1200);
        assert!(!third.capped);
    }

    #[test]
    fn dml_reports_the_affected_row_count() {
        let Some(conn) = connection() else {
            return skip();
        };

        // A no-op update inside the session's open transaction; it is never
        // committed, so the database is unchanged.
        let outcome = conn
            .execute("update dual set dummy = dummy where 1 = 0")
            .expect("no-op update");
        assert_eq!(outcome, StatementOutcome::RowCount(0));
    }
}
