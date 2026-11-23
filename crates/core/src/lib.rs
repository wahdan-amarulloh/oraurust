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
    Catalog, Column, Connection, DbError, MockConnection, Object, ObjectKind, Profile,
    ProfileStore, Result, ResultSet, StatementOutcome,
};
pub use script::{Statement, split, statement_at};
