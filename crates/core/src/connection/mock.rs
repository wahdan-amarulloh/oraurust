//! A canned [`Connection`] for tests: outcomes are scripted per statement.

use std::collections::VecDeque;

use super::*;

/// A scripted outcome, kept in a cloneable form so `execute(&self)` can hand
/// out a fresh one on every call (a [`Cursor`] itself is not cloneable).
#[derive(Debug, Clone)]
enum Scripted {
    Error(DbError),
    Rows {
        columns: Vec<Column>,
        /// One entry per page; `execute` starts a fresh cursor at the front.
        pages: Vec<Vec<Vec<Option<String>>>>,
    },
    RowCount(u64),
    Transaction(TransactionEnd),
}

impl Scripted {
    /// Builds the outcome for one `execute` call.
    fn outcome(&self) -> Result<StatementOutcome> {
        match self {
            Scripted::Error(error) => Err(error.clone()),
            Scripted::RowCount(count) => Ok(StatementOutcome::RowCount(*count)),
            Scripted::Transaction(end) => Ok(StatementOutcome::Transaction(*end)),
            Scripted::Rows { columns, pages } => {
                // Flatten the scripted pages so the cursor applies the same
                // `ROW_CAP` rule the real one does.
                let mut rows: VecDeque<Vec<Option<String>>> =
                    pages.iter().flatten().cloned().collect();
                // The first page is the size of the first scripted page,
                // clamped to the cap.
                let first_len = pages.first().map_or(0, Vec::len).min(ROW_CAP);
                let first: Vec<Vec<Option<String>>> = rows.drain(..first_len).collect();
                let fetched = first.len();
                let more = !rows.is_empty();
                let cursor = more.then(|| {
                    Box::new(MockCursor {
                        columns: columns.clone(),
                        rows,
                        fetched,
                    }) as Box<dyn Cursor>
                });
                Ok(StatementOutcome::Rows {
                    page: Page {
                        columns: columns.clone(),
                        rows: first,
                        fetched,
                        capped: false,
                        more,
                    },
                    cursor,
                })
            }
        }
    }
}

/// A [`Connection`] that returns scripted outcomes.
///
/// Outcomes are matched for every `execute` call: the first scripted entry
/// (in insertion order) whose statement text equals the statement wins; if
/// none, a scripted wildcard (empty statement text) applies; otherwise the
/// fallback outcome (default: `RowCount(1)`).
///
/// DDL/source/schema lookups are keyed by `Object` and default to an
/// [`DbError`] `ORA-04043`-style "not found" when unscripted.
#[derive(Debug)]
pub struct MockConnection {
    /// (statement text, outcome) pairs, matched in order. An empty statement
    /// text matches any statement — used by `with_select` for "next SELECT
    /// returns this".
    scripted: Vec<(String, Scripted)>,
    /// Fallback for statements with no scripted outcome.
    fallback: Scripted,
    ddl: Vec<(Object, String)>,
    source: Vec<(Object, String)>,
    catalog: Catalog,
}

impl Default for MockConnection {
    fn default() -> Self {
        Self {
            scripted: Vec::new(),
            fallback: Scripted::RowCount(1),
            ddl: Vec::new(),
            source: Vec::new(),
            catalog: Catalog::default(),
        }
    }
}

impl MockConnection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scripts `statement` to fail with an Oracle error, optionally with a
    /// 1-based error position inside the statement.
    pub fn with_error(
        mut self,
        statement: &str,
        code: &str,
        message: &str,
        position: Option<usize>,
    ) -> Self {
        self.scripted.push((
            statement.to_string(),
            Scripted::Error(DbError {
                code: code.to_string(),
                message: message.to_string(),
                position,
            }),
        ));
        self
    }

    /// Scripts the next statement with no closer scripted match to return a
    /// one-page Result Set with `columns` and `rows` (empty key = wildcard).
    ///
    /// Use [`MockConnection::with_select_for`] to key the outcome to one
    /// statement.
    pub fn with_select(self, columns: Vec<Column>, rows: Vec<Vec<Option<String>>>) -> Self {
        self.with_select_for("", columns, rows)
    }

    /// Scripts `statement` to return a one-page Result Set with `columns` and
    /// `rows`.
    ///
    /// To script a paged SELECT, use [`MockConnection::with_pages_for`].
    pub fn with_select_for(
        self,
        statement: &str,
        columns: Vec<Column>,
        rows: Vec<Vec<Option<String>>>,
    ) -> Self {
        self.with_pages_for(statement, columns, vec![rows])
    }

    /// Scripts `statement` to return a paged Result Set: `pages` are handed
    /// out one per `execute`/`Cursor::fetch` call, and a cursor is present
    /// only while pages remain.
    ///
    /// Each page's `fetched` is set from the total rows handed out so far, so
    /// the mock behaves like a real cursor.
    pub fn with_pages_for(
        mut self,
        statement: &str,
        columns: Vec<Column>,
        pages: Vec<Vec<Vec<Option<String>>>>,
    ) -> Self {
        self.scripted
            .push((statement.to_string(), Scripted::Rows { columns, pages }));
        self
    }

    /// Scripts the fallback outcome for statements with no scripted match.
    pub fn with_row_count(mut self, count: u64) -> Self {
        self.fallback = Scripted::RowCount(count);
        self
    }

    /// Scripts `statement` to end the session's Transaction (issue #7), as
    /// `COMMIT` / `ROLLBACK` do.
    pub fn with_transaction_end(mut self, statement: &str, end: TransactionEnd) -> Self {
        self.scripted
            .push((statement.to_string(), Scripted::Transaction(end)));
        self
    }

    /// Scripts the DDL returned by `fetch_ddl` for `object`.
    pub fn with_ddl(mut self, object: &Object, ddl: &str) -> Self {
        self.ddl.push((object.clone(), ddl.to_string()));
        self
    }

    /// Scripts the source returned by `fetch_source` for `object`.
    pub fn with_source(mut self, object: &Object, source: &str) -> Self {
        self.source.push((object.clone(), source.to_string()));
        self
    }

    /// Scripts the catalog returned by `describe_schema`.
    pub fn with_catalog(mut self, catalog: Catalog) -> Self {
        self.catalog = catalog;
        self
    }
}

impl Connection for MockConnection {
    fn execute(&self, statement: &str) -> Result<StatementOutcome> {
        // Exact scripted matches win over the wildcard, which wins over the
        // fallback.
        let scripted = self
            .scripted
            .iter()
            .find(|(text, _)| text == statement)
            .or_else(|| self.scripted.iter().find(|(text, _)| text.is_empty()))
            .map(|(_, scripted)| scripted)
            .unwrap_or(&self.fallback);
        scripted.outcome()
    }

    fn describe_schema(&self) -> Result<Catalog> {
        Ok(self.catalog.clone())
    }

    fn fetch_ddl(&self, object: &Object) -> Result<String> {
        self.ddl
            .iter()
            .find(|(o, _)| o == object)
            .map(|(_, ddl)| ddl.clone())
            .ok_or_else(|| not_found(object))
    }

    fn fetch_source(&self, object: &Object) -> Result<String> {
        self.source
            .iter()
            .find(|(o, _)| o == object)
            .map(|(_, src)| src.clone())
            .ok_or_else(|| not_found(object))
    }
}

/// The mock's cancel handle: cancelling succeeds and does nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockCancel;

impl CancelHandle for MockCancel {
    fn cancel(&self) -> Result<()> {
        Ok(())
    }
}

/// A scripted paged cursor: hands out its remaining pages one call each.
struct MockCursor {
    columns: Vec<Column>,
    /// Remaining rows, flattened across the scripted pages.
    rows: VecDeque<Vec<Option<String>>>,
    /// Rows handed out before this cursor was created.
    fetched: usize,
}

impl Cursor for MockCursor {
    fn fetch(&mut self, max_rows: usize) -> Result<Page> {
        // Same rules as the real cursor: never past `max_rows` or `ROW_CAP`.
        let want = max_rows.min(ROW_CAP.saturating_sub(self.fetched));
        let rows: Vec<Vec<Option<String>>> = self.rows.drain(..want.min(self.rows.len())).collect();
        self.fetched += rows.len();
        let capped = !self.rows.is_empty() && self.fetched >= ROW_CAP;
        Ok(Page {
            columns: self.columns.clone(),
            rows,
            fetched: self.fetched,
            capped,
            more: !self.rows.is_empty() && !capped,
        })
    }
}

fn not_found(object: &Object) -> DbError {
    DbError {
        code: "ORA-04043".into(),
        message: format!("object {} does not exist", object.name),
        position: None,
    }
}
