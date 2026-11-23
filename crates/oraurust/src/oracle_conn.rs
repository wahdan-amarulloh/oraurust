//! The real [`Connection`]: the `oracle` crate (ODPI-C) over Oracle Instant
//! Client, per ADR-0001. Built from a [`Profile`]'s Easy Connect string;
//! runs on the background thread (GPUI never blocks on Oracle).

use oraurust_core::{
    Catalog, Column, Connection, DbError, Object, ObjectKind, Profile, Result, ResultSet,
    StatementOutcome,
};

/// A live Oracle session behind the [`Connection`] seam.
pub struct OraConnection {
    session: oracle::Connection,
}

impl OraConnection {
    /// Connects using `profile`'s Easy Connect string.
    ///
    /// Errors are mapped through `instant_client::classify_error`, so a
    /// missing/incompatible Instant Client surfaces as an actionable message
    /// rather than a raw DPI- error.
    pub fn connect(profile: &Profile) -> Result<Self> {
        oracle::Connection::connect(
            &profile.username,
            &profile.password,
            profile.connect_string(),
        )
        .map(|session| Self { session })
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
    // `offset()` is ODPI-C's 0-based parse-error offset (0 when it has none),
    // so it becomes the 1-based position the UI reports (issue #6).
    let position = position.or_else(|| Some(dberr.offset() as usize).filter(|&p| p > 0));
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
            .build()
            .map_err(|e| map_error(e, None))?;
        // The statement type decides the outcome shape: SELECT → rows, DDL →
        // implicit commit, DML → affected row count in the open transaction.
        if stmt.is_query() {
            let mut result = stmt.query(&[]).map_err(|e| map_error(e, None))?;
            let columns: Vec<Column> = result
                .column_info()
                .iter()
                .map(|c| Column {
                    name: c.name().to_string(),
                    type_name: c.oracle_type().to_string(),
                })
                .collect();
            let mut rows: Vec<Vec<Option<String>>> = Vec::new();
            for row in result.by_ref() {
                let row = row.map_err(|e| map_error(e, None))?;
                let values = (1..=columns.len())
                    .map(|ix| row.get::<usize, Option<String>>(ix).map_err(|e| map_error(e, None)))
                    .collect::<Result<Vec<Option<String>>>>()?;
                rows.push(values);
            }
            Ok(StatementOutcome::Rows(ResultSet { columns, rows }))
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

    fn cancel(&self) -> Result<()> {
        // `break_execution` is an OCI call: it must also run off the UI
        // thread (the caller does that). The server raises ORA-01013 on the
        // interrupted fetch, which surfaces as the run's error.
        self.session.break_execution().map_err(|e| map_error(e, None))
    }

    fn describe_schema(&self) -> Result<Catalog> {
        let mut objects: Vec<Object> = Vec::new();
        let sql = "SELECT object_name, object_type \
                   FROM user_objects \
                   WHERE object_type IN (\
                     'TABLE','VIEW','PACKAGE','PACKAGE BODY','PROCEDURE',\
                     'FUNCTION','TRIGGER','SEQUENCE','SYNONYM','INDEX') \
                   ORDER BY object_type, object_name";
        for row in self.session.query(sql, &[]).map_err(|e| map_error(e, None))? {
            let row = row.map_err(|e| map_error(e, None))?;
            let name: String = row.get(1).map_err(|e| map_error(e, None))?;
            let kind_text: String = row.get(2).map_err(|e| map_error(e, None))?;
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
