//! Statement execution (issue #6): runs the statement under the cursor on the
//! background executor, exposes its outcome, and cancels a running statement.
//!
//! The UI never blocks on Oracle: the ODPI-C call happens inside
//! `background_spawn`. Cancel calls `Connection::cancel` (`break_execution`)
//! from a background task too, since it is an OCI call as well.

use std::sync::Arc;

use gpui_kit::*;
use oraurust_core::{DbError, ResultSet, StatementOutcome};

use crate::session::{Session, SharedConnection};

/// What the last (or in-flight) execution produced.
///
/// The Result Set is shared (`Arc`) with the Result Grid so showing a large
/// result does not copy every row on the UI thread.
#[derive(Clone, Debug, PartialEq)]
pub enum RunStatus {
    /// Nothing run yet.
    Idle,
    Running,
    /// A SELECT: a Result Set for the Result Grid.
    Rows(Arc<ResultSet>),
    /// DML: affected rows, still in the session's open Transaction.
    Dml(u64),
    /// DDL: succeeded with an implicit commit.
    Ddl,
    Failed {
        message: String,
        /// The user asked to cancel, so the Oracle cancel error is expected.
        cancelled: bool,
    },
}

impl RunStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, RunStatus::Running)
    }
}

/// Runs statements on the session and holds the outcome for the Result Grid.
pub struct QueryRunner {
    session: Entity<Session>,
    status: RunStatus,
    /// Bumped on every status change so observers can tell a re-run of the
    /// same statement from the previous result.
    generation: u64,
    /// The connection the running statement executes on, kept for cancel.
    running: Option<SharedConnection>,
    cancel_requested: bool,
}

impl QueryRunner {
    pub fn new(session: Entity<Session>) -> Self {
        Self {
            session,
            status: RunStatus::Idle,
            generation: 0,
            running: None,
            cancel_requested: false,
        }
    }

    pub fn status(&self) -> &RunStatus {
        &self.status
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Executes `statement` off the UI thread. A second run while one is in
    /// flight is ignored.
    pub fn run(&mut self, statement: String, cx: &mut Context<Self>) {
        if self.status.is_running() {
            return;
        }
        let Some(connection) = self.session.read(cx).connection() else {
            self.set_status(
                RunStatus::Failed {
                    message: "Not connected. Connect to a profile first.".into(),
                    cancelled: false,
                },
                cx,
            );
            return;
        };

        self.running = Some(connection.clone());
        self.cancel_requested = false;
        self.set_status(RunStatus::Running, cx);

        cx.spawn(async move |this, cx| {
            // ODPI-C is blocking: the whole execute + fetch runs off the UI.
            let outcome = cx
                .background_spawn(async move { connection.execute(&statement) })
                .await;
            this.update(cx, |this, cx| this.finish(outcome, cx)).ok();
        })
        .detach();
    }

    /// Asks Oracle to break the running statement (issue #6's cancel control).
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(connection) = self.running.clone() else {
            return;
        };
        if self.cancel_requested {
            return;
        }
        self.cancel_requested = true;
        cx.background_spawn(async move {
            let _ = connection.cancel();
        })
        .detach();
    }

    fn finish(
        &mut self,
        outcome: std::result::Result<StatementOutcome, DbError>,
        cx: &mut Context<Self>,
    ) {
        let cancelled = std::mem::take(&mut self.cancel_requested);
        self.running = None;
        let status = match outcome {
            Ok(StatementOutcome::Rows(result)) => RunStatus::Rows(Arc::new(result)),
            Ok(StatementOutcome::RowCount(affected)) => RunStatus::Dml(affected),
            Ok(StatementOutcome::Ddl) => RunStatus::Ddl,
            Err(error) => RunStatus::Failed {
                message: error.to_string(),
                cancelled,
            },
        };
        self.set_status(status, cx);
    }

    fn set_status(&mut self, status: RunStatus, cx: &mut Context<Self>) {
        self.status = status;
        self.generation += 1;
        cx.notify();
    }
}
