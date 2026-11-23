//! The session's Transaction state, derived from execution outcomes
//! (issue #7: "Nothing auto-commits; commit and rollback buttons control the
//! session transaction").

use oraurust_core::{
    Column, Page, StatementOutcome, TransactionEffect, TransactionEnd, TransactionState,
};

fn select() -> StatementOutcome {
    StatementOutcome::Rows {
        page: Page {
            columns: vec![Column {
                name: "DUMMY".into(),
                type_name: "VARCHAR2".into(),
            }],
            rows: vec![vec![Some("X".into())]],
            fetched: 1,
            capped: false,
            more: false,
        },
        cursor: None,
    }
}

fn effect_of(outcome: &StatementOutcome) -> TransactionEffect {
    TransactionEffect::of_outcome(outcome)
}

#[test]
fn dml_makes_the_transaction_dirty() {
    let mut transaction = TransactionState::Clean;

    transaction.apply(effect_of(&StatementOutcome::RowCount(3)));

    assert!(transaction.is_dirty());
    assert_eq!(transaction, TransactionState::Dirty);
}

#[test]
fn a_select_leaves_the_transaction_unchanged() {
    let mut clean = TransactionState::Clean;
    clean.apply(effect_of(&select()));
    assert_eq!(clean, TransactionState::Clean);

    let mut dirty = TransactionState::Dirty;
    dirty.apply(effect_of(&select()));
    assert_eq!(
        dirty,
        TransactionState::Dirty,
        "a read never ends a transaction"
    );
}

#[test]
fn a_second_dml_stays_dirty() {
    let mut transaction = TransactionState::Clean;
    transaction.apply(effect_of(&StatementOutcome::RowCount(1)));
    transaction.apply(effect_of(&StatementOutcome::RowCount(0)));
    assert_eq!(transaction, TransactionState::Dirty);
}

#[test]
fn ddl_commits_implicitly_and_leaves_the_transaction_clean() {
    let mut transaction = TransactionState::Dirty;

    transaction.apply(effect_of(&StatementOutcome::Ddl));

    assert_eq!(transaction, TransactionState::Clean);
}

#[test]
fn commit_and_rollback_leave_the_transaction_clean() {
    for end in [TransactionEnd::Commit, TransactionEnd::Rollback] {
        let mut transaction = TransactionState::Dirty;

        transaction.apply(effect_of(&StatementOutcome::Transaction(end)));

        assert_eq!(
            transaction,
            TransactionState::Clean,
            "{end:?} ends the transaction"
        );
    }
}

#[test]
fn commit_on_a_clean_transaction_stays_clean() {
    let mut transaction = TransactionState::Clean;

    transaction.apply(effect_of(&StatementOutcome::Transaction(
        TransactionEnd::Commit,
    )));

    assert_eq!(transaction, TransactionState::Clean);
}
