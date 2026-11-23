//! UI-free logic core for oraurust.
//!
//! The only boundaries across which logic is tested (spec: issue #1):
//!
//! - [`script::split`] — pure: script text → statements.
//! - [`script::statement_at`] — pure: the statement under a cursor offset.
//! - [`connection::Connection`] — the app ↔ database seam, with a mocked
//!   implementation for tests.

pub mod connection;
pub mod csv;
pub mod instant_client;
pub mod script;
pub mod transaction;

pub use connection::{
    CancelHandle, Catalog, Column, Connection, Cursor, DbError, FETCH_ARRAY_SIZE, FETCH_ROWS,
    MockCancel, MockConnection, Object, ObjectKind, Page, Profile, ProfileStore, ROW_CAP, Result,
    ResultSet, StatementOutcome,
};
pub use csv::to_csv;
pub use script::{Statement, split, statement_at};
pub use transaction::{TransactionEffect, TransactionEnd, TransactionState};
