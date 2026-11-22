//! A canned [`Connection`] for tests: outcomes are scripted per statement.

use super::*;

/// A [`Connection`] that returns scripted outcomes.
///
/// Outcomes are matched in this order for every `execute` call:
/// 1. a scripted [`DbError`] whose statement text matches,
/// 2. a scripted [`StatementOutcome::Rows`] whose statement text matches,
/// 3. otherwise the fallback outcome (default: `RowCount(1)`).
///
/// DDL/source/schema lookups are keyed by `Object` and default to an
/// [`DbError`] `ORA-04043`-style "not found" when unscripted.
#[derive(Debug, Clone)]
pub struct MockConnection {
    /// (statement text, outcome) pairs, matched in order. An empty statement
    /// text matches any statement — used by `with_select` for "next SELECT
    /// returns this".
    scripted: Vec<(String, Result<StatementOutcome>)>,
    /// Fallback for statements with no scripted outcome.
    fallback: Option<Result<StatementOutcome>>,
    ddl: Vec<(Object, String)>,
    source: Vec<(Object, String)>,
    catalog: Catalog,
}

impl Default for MockConnection {
    fn default() -> Self {
        Self {
            scripted: Vec::new(),
            fallback: Some(Ok(StatementOutcome::RowCount(1))),
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
            Err(DbError {
                code: code.to_string(),
                message: message.to_string(),
                position,
            }),
        ));
        self
    }

    /// Scripts the next statement with no closer scripted match to return a
    /// Result Set with `columns` and `rows` (empty key = wildcard).
    ///
    /// Use [`MockConnection::with_select_for`] to key the outcome to one
    /// statement.
    pub fn with_select(
        self,
        columns: Vec<Column>,
        rows: Vec<Vec<Option<String>>>,
    ) -> Self {
        self.with_select_for("", columns, rows)
    }

    /// Scripts `statement` to return a Result Set with `columns` and `rows`.
    pub fn with_select_for(
        mut self,
        statement: &str,
        columns: Vec<Column>,
        rows: Vec<Vec<Option<String>>>,
    ) -> Self {
        self.scripted.push((
            statement.to_string(),
            Ok(StatementOutcome::Rows(ResultSet { columns, rows })),
        ));
        self
    }

    /// Scripts the fallback outcome for statements with no scripted match.
    pub fn with_row_count(mut self, count: u64) -> Self {
        self.fallback = Some(Ok(StatementOutcome::RowCount(count)));
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
    fn execute(&mut self, statement: &str) -> Result<StatementOutcome> {
        // Exact scripted matches win over the wildcard, which wins over the
        // fallback.
        for (text, outcome) in &self.scripted {
            if text == statement {
                return outcome.clone();
            }
        }
        if let Some((_, outcome)) = self.scripted.iter().find(|(text, _)| text.is_empty()) {
            return outcome.clone();
        }
        self.fallback
            .clone()
            .expect("MockConnection always has a fallback")
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

fn not_found(object: &Object) -> DbError {
    DbError {
        code: "ORA-04043".into(),
        message: format!("object {} does not exist", object.name),
        position: None,
    }
}
