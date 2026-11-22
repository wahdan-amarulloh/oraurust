//! UI-free logic core for oraurust.
//!
//! The only boundaries across which logic is tested (spec: issue #1):
//!
//! - [`script::split`] — pure: script text → statements.
//! - [`connection::Connection`] — the app ↔ database seam, with a mocked
//!   implementation for tests.

pub mod connection;
pub mod script;

pub use connection::{
    Catalog, Column, Connection, DbError, MockConnection, Object, ObjectKind, Result, ResultSet,
    StatementOutcome,
};
pub use script::{Statement, split};
