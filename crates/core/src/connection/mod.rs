//! The Connection seam: the boundary between the app and the database.
//!
//! Per ADR-0001, all Oracle-specific code (the `oracle` crate over ODPI-C,
//! dictionary SQL, `DBMS_METADATA`) will live behind this trait; the real
//! implementation runs on a background thread. The mock here lets the rest
//! of the app be built and tested without a database.

mod mock;
mod profile;
mod store;

pub use mock::{MockCancel, MockConnection};
pub use profile::Profile;
pub use store::ProfileStore;

/// The app ↔ database boundary. All Oracle-specific code lives behind it.
///
/// Four methods, as the spec fixes it (issue #1). `execute` takes `&self` so
/// it can run on a Session's worker thread while the UI keeps a separate
/// [`CancelHandle`] to break it (ADR-0002).
pub trait Connection: Send + Sync {
    /// Executes one Statement. A SELECT yields a [`StatementOutcome::Rows`]
    /// first page (plus a cursor when more rows remain), DML a
    /// [`StatementOutcome::RowCount`] within the session's open Transaction,
    /// DDL a [`StatementOutcome::Ddl`] (implicit commit).
    fn execute(&self, statement: &str) -> Result<StatementOutcome>;

    /// Loads the Catalog: the connected user's visible objects.
    fn describe_schema(&self) -> Result<Catalog>;

    /// Fetches an Object's DDL via `DBMS_METADATA.GET_DDL`.
    fn fetch_ddl(&self, object: &Object) -> Result<String>;

    /// Fetches a program unit's source via `ALL_SOURCE`.
    fn fetch_source(&self, object: &Object) -> Result<String>;
}

/// Breaks the statement running on a connection (issue #6's cancel control).
///
/// Cancel cannot go through [`Connection::execute`], which is blocked for the
/// whole statement; a driver hands out a second handle over the same session
/// that calls `break_execution` (ADR-0002). Obtained from the concrete driver
/// (`OraConnection::cancel_handle`), not from the four-method seam.
pub trait CancelHandle: Send + Sync {
    fn cancel(&self) -> Result<()>;
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

impl DbError {
    /// Builds an error, removing a leading copy of `code` from `message`.
    ///
    /// ODPI-C messages already carry their own `ORA-nnnnn: ` prefix, so
    /// storing both verbatim would double it in [`Display`](std::fmt::Display).
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        position: Option<usize>,
    ) -> Self {
        let code = code.into();
        let message = message.into();
        let message = strip_code_prefix(&message, &code)
            .map(str::to_string)
            .unwrap_or(message);
        Self {
            code,
            message,
            position,
        }
    }
}

/// `message` without a leading `<code>: ` (and without rust-oracle's
/// `"OCI Error: "` prefix), when it duplicates the code.
fn strip_code_prefix<'a>(message: &'a str, code: &str) -> Option<&'a str> {
    if code.is_empty() {
        return None;
    }
    let message = message.strip_prefix("OCI Error: ").unwrap_or(message);
    let rest = message.strip_prefix(code)?;
    rest.strip_prefix(": ")
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
pub enum StatementOutcome {
    /// A SELECT: the first page of its Result Set, plus a cursor when more
    /// rows remain beyond the first page.
    Rows {
        page: Page,
        cursor: Option<Box<dyn Cursor>>,
    },
    /// DML: affected row count; the session's Transaction stays open.
    RowCount(u64),
    /// DDL: succeeded with an implicit commit (Oracle semantics).
    Ddl,
}

impl std::fmt::Debug for StatementOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatementOutcome::Rows { page, cursor } => f
                .debug_struct("Rows")
                .field("page", page)
                .field("has_cursor", &cursor.is_some())
                .finish(),
            StatementOutcome::RowCount(count) => f.debug_tuple("RowCount").field(count).finish(),
            StatementOutcome::Ddl => f.write_str("Ddl"),
        }
    }
}

impl PartialEq for StatementOutcome {
    /// Compares outcomes by what they report, not by cursor identity: two
    /// first pages are equal when their page and "more rows remain" agree.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                StatementOutcome::Rows { page, cursor },
                StatementOutcome::Rows {
                    page: other_page,
                    cursor: other_cursor,
                },
            ) => page == other_page && cursor.is_some() == other_cursor.is_some(),
            (StatementOutcome::RowCount(count), StatementOutcome::RowCount(other_count)) => {
                count == other_count
            }
            (StatementOutcome::Ddl, StatementOutcome::Ddl) => true,
            _ => false,
        }
    }
}

/// One page of a Result Set: its columns and the rows fetched for it.
///
/// A SELECT is fetched in pages of [`FETCH_ARRAY_SIZE`] rows, up to
/// [`ROW_CAP`] in all, so the UI never materialises an unbounded result
/// (issue #6).
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// Column metadata; every page of one cursor carries the same columns.
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<String>>>,
    /// Rows fetched so far across every page of the Result Set, including
    /// this one — the grid's "N rows fetched" line.
    pub fetched: usize,
    /// True when the row cap stopped the fetch before the Result Set ended.
    pub capped: bool,
    /// True when the Result Set has rows left beyond this page.
    pub more: bool,
}

/// Rows requested per Oracle round trip (ODPI-C `fetch_array_size`).
///
/// The driver wants a `u32`; [`FETCH_ROWS`] is the same value as the `usize`
/// the cursor API uses, so call sites never cast.
pub const FETCH_ARRAY_SIZE: u32 = 500;

/// [`FETCH_ARRAY_SIZE`] as a `usize` for [`Cursor::fetch`] call sites.
pub const FETCH_ROWS: usize = FETCH_ARRAY_SIZE as usize;

/// Rows a Result Set may accumulate before the cursor stops fetching.
pub const ROW_CAP: usize = 10_000;

/// Continues a paged SELECT after its first [`Page`].
///
/// Fetches on the Session's worker thread; dropping it releases the
/// statement's server-side resources.
pub trait Cursor: Send {
    /// Fetches up to `max_rows` more rows, stopping at [`ROW_CAP`] in total.
    /// The returned page's `fetched` is cumulative.
    fn fetch(&mut self, max_rows: usize) -> Result<Page>;
}

/// A [`Cursor`]'s rows accumulated for the Result Grid.
///
/// The app appends each [`Page`]'s rows here; the grid renders from it. Kept
/// out of the seam's return type so a SELECT never has to materialise every
/// row at once (ADR-0002).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultSet {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<String>>>,
}

impl ResultSet {
    /// A Result Set holding exactly one [`Page`]'s rows.
    pub fn from_page(page: Page) -> Self {
        Self {
            columns: page.columns,
            rows: page.rows,
        }
    }

    /// Appends a fetched [`Page`]'s rows to this Result Set.
    pub fn extend_page(&mut self, page: Page) {
        self.rows.extend(page.rows);
    }
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
