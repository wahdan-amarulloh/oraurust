//! UI-free logic core for oraurust.
//!
//! The only boundaries across which logic is tested (spec: issue #1):
//!
//! - [`script::split`] — pure: script text → statements.
//! - [`script::statement_at`] — pure: the statement under a cursor offset.
//! - [`connection::Connection`] — the app ↔ database seam, with a mocked
//!   implementation for tests.

pub mod connection;
pub mod instant_client;
pub mod script;

pub use connection::{
    CancelHandle, Catalog, Column, Connection, Cursor, DbError, MockCancel, MockConnection,
    Object, ObjectKind, Page, Profile, ProfileStore, Result, ResultSet, StatementOutcome,
    FETCH_ARRAY_SIZE, FETCH_ROWS, ROW_CAP,
};
pub use script::{Statement, split, statement_at};
