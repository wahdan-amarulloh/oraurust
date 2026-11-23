//! The session's Transaction, and the rule that derives its state from
//! execution outcomes (issue #7).
//!
//! Oracle opens a transaction implicitly on the first DML and keeps it open
//! until COMMIT, ROLLBACK, or a DDL statement (which commits implicitly). The
//! app never auto-commits: [`TransactionState`] only records whether changes
//! are pending, so the UI can show it and offer Commit/Rollback.
//!
//! Rules (see `GLOSSARY.md`, *Transaction*):
//!
//! - DML → dirty; the transaction stays open.
//! - DDL → clean; Oracle committed implicitly. Every statement that is not a
//!   query, DML, PL/SQL, COMMIT or ROLLBACK (TRUNCATE, GRANT, COMMENT, …)
//!   arrives as DDL, because those also commit implicitly.
//! - COMMIT / ROLLBACK → clean.
//! - SELECT → unchanged; a read neither opens nor ends a transaction.
//!
//! A PL/SQL block reports a row count, so it is treated as DML: it may have
//! run DML, and a block that commits internally is rare enough that a stale
//! "uncommitted" indicator is safer than a missing one.

use crate::connection::StatementOutcome;

/// A statement that ends the session's transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionEnd {
    Commit,
    Rollback,
}

impl TransactionEnd {
    /// The SQL statement the Commit/Rollback button sends to the database.
    pub fn statement(self) -> &'static str {
        match self {
            TransactionEnd::Commit => "COMMIT",
            TransactionEnd::Rollback => "ROLLBACK",
        }
    }

    /// How the Result Grid's status line reports it.
    pub fn label(self) -> &'static str {
        match self {
            TransactionEnd::Commit => "Committed.",
            TransactionEnd::Rollback => "Rolled back.",
        }
    }
}

/// What executing a statement did to the session's Transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionEffect {
    /// The statement neither opened nor ended a transaction (SELECT).
    Unchanged,
    /// The statement left changes pending (DML, a PL/SQL block).
    Dirty,
    /// The statement ended the transaction (DDL, COMMIT, ROLLBACK).
    Clean,
}

impl TransactionEffect {
    /// The effect of one executed statement, from what it reported.
    pub fn of_outcome(outcome: &StatementOutcome) -> Self {
        match outcome {
            StatementOutcome::Rows { .. } => TransactionEffect::Unchanged,
            StatementOutcome::RowCount(_) => TransactionEffect::Dirty,
            StatementOutcome::Ddl | StatementOutcome::Transaction(_) => TransactionEffect::Clean,
        }
    }
}

/// Whether the session has uncommitted changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TransactionState {
    /// No changes pending: nothing to commit or roll back.
    #[default]
    Clean,
    /// At least one DML statement is uncommitted.
    Dirty,
}

impl TransactionState {
    /// Applies one statement's effect to this state.
    pub fn apply(&mut self, effect: TransactionEffect) {
        match effect {
            TransactionEffect::Unchanged => {}
            TransactionEffect::Dirty => *self = TransactionState::Dirty,
            TransactionEffect::Clean => *self = TransactionState::Clean,
        }
    }

    pub fn is_dirty(self) -> bool {
        matches!(self, TransactionState::Dirty)
    }

    pub fn is_clean(self) -> bool {
        matches!(self, TransactionState::Clean)
    }

    /// The indicator the UI shows for this state.
    pub fn label(self) -> &'static str {
        match self {
            TransactionState::Clean => "No uncommitted changes",
            TransactionState::Dirty => "Uncommitted changes",
        }
    }
}
