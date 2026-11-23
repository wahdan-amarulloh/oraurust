//! The Connection seam: the boundary between the app and the database.
//!
//! Per ADR-0001, all Oracle-specific code (the `oracle` crate over ODPI-C,
//! dictionary SQL, `DBMS_METADATA`) will live behind this trait; the real
//! implementation runs on a background thread. The mock here lets the rest
//! of the app be built and tested without a database.

mod mock;
mod profile;
mod store;

pub use mock::MockConnection;
pub use profile::Profile;
pub use store::ProfileStore;

/// The app ↔ database boundary. All Oracle-specific code lives behind it.
///
/// `Send + Sync` so an `Arc<dyn Connection>` can be handed to the background
/// executor while the UI keeps a handle to cancel (issue #6).
pub trait Connection: Send + Sync {
    /// Executes one Statement. A SELECT yields a [`StatementOutcome::Rows`],
    /// DML a [`StatementOutcome::RowCount`] within the session's open
    /// Transaction, DDL a [`StatementOutcome::Ddl`] (implicit commit).
    ///
    /// Takes `&self` so execution can run on a background thread while the UI
    /// still holds the connection — to cancel the running statement.
    fn execute(&self, statement: &str) -> Result<StatementOutcome>;

    /// Cancels the statement running on this connection, if any (issue #6's
    /// cancel control). The default is a no-op for connections that cannot
    /// cancel; Oracle implements it with `break_execution`.
    fn cancel(&self) -> Result<()> {
        Ok(())
    }

    /// Loads the Catalog: the connected user's visible objects.
    fn describe_schema(&self) -> Result<Catalog>;

    /// Fetches an Object's DDL via `DBMS_METADATA.GET_DDL`.
    fn fetch_ddl(&self, object: &Object) -> Result<String>;

    /// Fetches a program unit's source via `ALL_SOURCE`.
    fn fetch_source(&self, object: &Object) -> Result<String>;
}

/// A database result.
pub type Result<T> = std::result::Result<T, DbError>;

/// An Oracle error: `ORA-` code with message and, when known, position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbError {
    /// Oracle error code, e.g. `"ORA-00942"`; empty for non-ORA errors.
    pub code: String,
    pub message: String,
    /// 1-based offset into the failing statement, when Oracle reports one.
    pub position: Option<usize>,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.code.is_empty() {
            write!(f, "{}", self.message)?;
        } else {
            write!(f, "{}: {}", self.code, self.message)?;
        }
        if let Some(position) = self.position {
            write!(f, " (at position {position})")?;
        }
        Ok(())
    }
}

impl std::error::Error for DbError {}

/// What executing one Statement produced.
#[derive(Debug, Clone, PartialEq)]
pub enum StatementOutcome {
    /// A SELECT: column metadata plus rows (a Result Set).
    Rows(ResultSet),
    /// DML: affected row count; the session's Transaction stays open.
    RowCount(u64),
    /// DDL: succeeded with an implicit commit (Oracle semantics).
    Ddl,
}

/// Tabular output of a SELECT: column metadata plus rows.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultSet {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<String>>>,
}

/// One column of a Result Set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// Oracle type name as reported by describe, e.g. `NUMBER`, `VARCHAR2`.
    pub type_name: String,
}

/// A dictionary entity visible in the Schema Browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub name: String,
    pub kind: ObjectKind,
}

/// The kind of an Object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Table,
    View,
    Package,
    PackageBody,
    Procedure,
    Function,
    Trigger,
    Sequence,
    Synonym,
    Index,
}

impl ObjectKind {
    /// Display grouping label used by the Schema Browser tree.
    pub fn label(self) -> &'static str {
        match self {
            ObjectKind::Table => "Tables",
            ObjectKind::View => "Views",
            ObjectKind::Package => "Packages",
            ObjectKind::PackageBody => "Package Bodies",
            ObjectKind::Procedure => "Procedures",
            ObjectKind::Function => "Functions",
            ObjectKind::Trigger => "Triggers",
            ObjectKind::Sequence => "Sequences",
            ObjectKind::Synonym => "Synonyms",
            ObjectKind::Index => "Indexes",
        }
    }
}

/// The in-memory cache of Schema Browser metadata used for completion.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    pub objects: Vec<Object>,
}
