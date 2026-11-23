//! Statement execution (issue #6): runs the statement under the cursor on the
//! session's worker thread, accumulates a SELECT's pages, and cancels a
//! running statement. Issue #7 adds the session's Transaction state, derived
//! from each outcome by core's rule, and explicit Commit/Rollback through
//! `execute`.
//!
//! The UI never blocks on Oracle: every database call goes to
//! [`SessionWorker`]'s thread. A SELECT arrives as a first page plus a cursor
//! the worker holds; `fetch_more` pulls further pages up to the row cap
//! (ADR-0002).

use std::sync::Arc;

use gpui_kit::*;
use oraurust_core::{DbError, Page, ResultSet, TransactionEnd, TransactionState};

use crate::session::{Executed, ExecutedOutcome, Session};

/// The status line shown when a database call is attempted disconnected.
const NOT_CONNECTED: &str = "Not connected. Connect to a profile first.";

/// What the last (or in-flight) execution produced.
///
/// The accumulated Result Set is shared (`Arc`) with the Result Grid so
/// showing a large result does not copy every row on the UI thread.
#[derive(Clone, Debug, PartialEq)]
pub enum RunStatus {
    /// Nothing run yet.
    Idle,
    Running,
    /// A SELECT: the rows fetched so far, and whether more can be fetched.
    Rows {
        result: Arc<ResultSet>,
        more: bool,
        capped: bool,
    },
    /// DML: affected rows, still in the session's open Transaction.
    Dml(u64),
    /// DDL: succeeded with an implicit commit.
    Ddl,
    /// COMMIT or ROLLBACK succeeded; the Transaction is clean.
    Transaction(TransactionEnd),
    Failed {
        message: String,
        /// The user asked to cancel, so the Oracle cancel error is expected.
        cancelled: bool,
    },
}

impl RunStatus {
    /// Whether a further page can be fetched (a SELECT with more rows, under
    /// the cap, not already fetching).
    pub fn can_fetch_more(&self) -> bool {
        matches!(
            self,
            RunStatus::Rows {
                more: true,
                capped: false,
                ..
            }
        )
    }
}

/// Runs statements on the session and holds the outcome for the Result Grid.
pub struct QueryRunner {
    session: Entity<Session>,
    status: RunStatus,
    /// Whether the session has uncommitted changes (issue #7). Derived from
    /// each execution's outcome; never set by the app itself, because the app
    /// never commits.
    transaction: TransactionState,
    /// The last failed Commit/Rollback, shown beside the Transaction state so
    /// the user knows the transaction is still open.
    end_error: Option<DbError>,
    /// Bumped on every status change so observers can tell a re-run of the
    /// same statement from the previous result.
    generation: u64,
    /// True while a statement (or a page fetch) is in flight.
    busy: bool,
    cancel_requested: bool,
    _observed_session: Subscription,
}

impl QueryRunner {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        // A new or lost session has no transaction of its own: forget the
        // previous one so a stale "uncommitted" is never shown against it.
        let observed = cx.observe(&session, |this, session, cx| {
            if !session.read(cx).state().is_connected() {
                this.transaction = TransactionState::Clean;
                this.end_error = None;
                cx.notify();
            }
        });
        Self {
            session,
            status: RunStatus::Idle,
            transaction: TransactionState::default(),
            end_error: None,
            generation: 0,
            busy: false,
            cancel_requested: false,
            _observed_session: observed,
        }
    }

    pub fn status(&self) -> &RunStatus {
        &self.status
    }

    /// Whether the session has uncommitted changes (issue #7).
    pub fn transaction(&self) -> TransactionState {
        self.transaction
    }

    /// The last failed Commit/Rollback, when one failed. A success is shown in
    /// the status line instead, so this only holds a problem worth repeating.
    pub fn end_error(&self) -> Option<&DbError> {
        self.end_error.as_ref()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// True while any database call is in flight.
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Executes `statement` off the UI thread. A second run while one is in
    /// flight is ignored.
    pub fn run(&mut self, statement: String, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(worker) = self.session.read(cx).worker() else {
            self.set_status(
                RunStatus::Failed {
                    message: NOT_CONNECTED.into(),
                    cancelled: false,
                },
                cx,
            );
            return;
        };

        self.busy = true;
        self.cancel_requested = false;
        self.set_status(RunStatus::Running, cx);

        cx.spawn(async move |this, cx| {
            let outcome = worker.execute(statement).await;
            this.update(cx, |this, cx| this.finish(outcome, cx)).ok();
        })
        .detach();
    }

    /// Ends the session's Transaction (issue #7): runs `COMMIT` or `ROLLBACK`
    /// as a statement, so the command travels the same serialised path as
    /// every other Oracle call. The transaction state changes only once the
    /// server confirms it, and a failure is reported rather than assumed.
    pub fn end_transaction(&mut self, end: TransactionEnd, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(worker) = self.session.read(cx).worker() else {
            self.set_status(
                RunStatus::Failed {
                    message: NOT_CONNECTED.into(),
                    cancelled: false,
                },
                cx,
            );
            return;
        };
        let statement = end.statement();
        self.busy = true;
        self.cancel_requested = false;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let outcome = worker.execute(statement.to_string()).await;
            this.update(cx, |this, cx| this.finish_end(end, outcome, cx))
                .ok();
        })
        .detach();
    }

    /// Records the Commit/Rollback result, deriving both the state and the
    /// status from what the worker reported — not from what was requested, so
    /// a server that classified the statement differently is shown as it
    /// actually ran.
    ///
    /// On success, a status that said "uncommitted" stops saying it: a DML
    /// result becomes the grid's "Committed."/"Rolled back." line, while a
    /// SELECT's Result Set is kept (its rows are still worth showing) with
    /// "fetch more" switched off, because the worker dropped the cursor when
    /// the transaction statement ran.
    fn finish_end(
        &mut self,
        end: TransactionEnd,
        outcome: Result<Executed, DbError>,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        self.cancel_requested = false;
        match outcome {
            Ok(executed) => {
                self.transaction.apply(executed.effect);
                let reported = match executed.outcome {
                    ExecutedOutcome::Transaction(reported) => reported,
                    // The server did not treat it as COMMIT/ROLLBACK; claim
                    // nothing beyond the state its effect produced.
                    _ => end,
                };
                // Only claim "Committed."/"Rolled back." over the reported
                // end; a "Committed." line over a still-dirty transaction
                // would lie.
                self.status = match (&self.status, reported) {
                    (RunStatus::Rows { result, capped, .. }, _) => RunStatus::Rows {
                        result: result.clone(),
                        more: false,
                        capped: *capped,
                    },
                    (_, reported) if self.transaction.is_clean() => {
                        RunStatus::Transaction(reported)
                    }
                    (status, _) => status.clone(),
                };
                self.end_error = None;
            }
            Err(error) => self.end_error = Some(error),
        }
        self.generation += 1;
        cx.notify();
    }

    /// Fetches the next page of the current Result Set. Called after a page
    /// arrives so a SELECT keeps fetching until it ends or hits the cap.
    fn fetch_more(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let RunStatus::Rows { result, .. } = &self.status else {
            return;
        };
        if !self.status.can_fetch_more() {
            return;
        }
        let Some(worker) = self.session.read(cx).worker() else {
            return;
        };
        let result = result.clone();

        self.busy = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let page = worker.fetch_more().await;
            this.update(cx, |this, cx| this.append(result, page, cx))
                .ok();
        })
        .detach();
    }

    /// Asks Oracle to break the running statement (issue #6's cancel control).
    ///
    /// Cancel goes through the session's [`CancelHandle`], not the worker's
    /// queue, which is blocked while `execute` runs. `cancel_requested` is
    /// set only once the break succeeds, so a failed break can be retried.
    ///
    /// Limitation: if the worker has not yet started the statement (it is
    /// still reading the command off the queue), `break_execution` succeeds
    /// against nothing and the statement then runs to completion. The window
    /// is one command's queue depth, so it is only reachable by cancelling in
    /// the same tick as Run; a user cannot hit it in practice.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.cancel_requested || !self.busy {
            return;
        }
        let Some(worker) = self.session.read(cx).worker() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            // `break_execution` is an OCI call: it runs off the UI thread too.
            let broke = cx.background_spawn(async move { worker.cancel() }).await;
            this.update(cx, |this, cx| {
                if broke.is_ok() {
                    this.cancel_requested = true;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish(&mut self, outcome: std::result::Result<Executed, DbError>, cx: &mut Context<Self>) {
        let cancelled = std::mem::take(&mut self.cancel_requested);
        self.busy = false;
        if let Ok(executed) = &outcome {
            self.end_error = None;
            self.transaction.apply(executed.effect);
        }
        let status = match outcome {
            Ok(Executed {
                outcome: ExecutedOutcome::Rows { page },
                ..
            }) => {
                let capped = page.capped;
                let more = page.more && !capped;
                let status = RunStatus::Rows {
                    result: Arc::new(ResultSet::from_page(page)),
                    more,
                    capped,
                };
                self.set_status(status, cx);
                // Keep fetching pages until the Result Set ends or hits the
                // cap (issue #6's paged fetch).
                if more {
                    self.fetch_more(cx);
                }
                return;
            }
            Ok(Executed {
                outcome: ExecutedOutcome::RowCount(affected),
                ..
            }) => RunStatus::Dml(affected),
            Ok(Executed {
                outcome: ExecutedOutcome::Ddl,
                ..
            }) => RunStatus::Ddl,
            Ok(Executed {
                outcome: ExecutedOutcome::Transaction(end),
                ..
            }) => RunStatus::Transaction(end),
            Err(error) => RunStatus::Failed {
                message: error.to_string(),
                cancelled,
            },
        };
        self.set_status(status, cx);
    }

    /// Appends a fetched page to the grid's Result Set, then keeps fetching
    /// pages until the Result Set ends or the row cap is hit (issue #6: a
    /// SELECT is fetched in pages, not one page per click).
    ///
    /// The page's rows are moved, not copied; only the already-shown rows are
    /// cloned, so the UI-thread work per page is one bounded copy of the rows
    /// shown so far.
    fn append(
        &mut self,
        previous: Arc<ResultSet>,
        page: std::result::Result<Page, DbError>,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        match page {
            Ok(page) => {
                let mut rows = Vec::with_capacity(previous.rows.len() + page.rows.len());
                rows.extend(previous.rows.iter().cloned());
                rows.extend(page.rows);
                let status = RunStatus::Rows {
                    result: Arc::new(ResultSet {
                        columns: page.columns,
                        rows,
                    }),
                    more: page.more,
                    capped: page.capped,
                };
                self.set_status(status, cx);
                if page.more && !page.capped {
                    self.fetch_more(cx);
                }
            }
            Err(error) => {
                let cancelled = std::mem::take(&mut self.cancel_requested);
                // A cancel stops a page fetch, not the statement: keep the
                // rows already fetched instead of wiping the grid. The break
                // may have aborted the statement, so its cursor is gone and
                // "fetch more" stops being offered.
                if cancelled && let RunStatus::Rows { result, capped, .. } = &self.status {
                    let kept = RunStatus::Rows {
                        result: result.clone(),
                        more: false,
                        capped: *capped,
                    };
                    self.set_status(kept, cx);
                    return;
                }
                self.set_status(
                    RunStatus::Failed {
                        message: error.to_string(),
                        cancelled,
                    },
                    cx,
                );
            }
        }
    }

    fn set_status(&mut self, status: RunStatus, cx: &mut Context<Self>) {
        self.status = status;
        self.generation += 1;
        cx.notify();
    }
}
