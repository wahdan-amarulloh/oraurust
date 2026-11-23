//! Statement execution (issue #6): runs the statement under the cursor on the
//! session's worker thread, accumulates a SELECT's pages, and cancels a
//! running statement.
//!
//! The UI never blocks on Oracle: every database call goes to
//! [`SessionWorker`]'s thread. A SELECT arrives as a first page plus a cursor
//! the worker holds; `fetch_more` pulls further pages up to the row cap
//! (ADR-0002).

use std::sync::Arc;

use gpui_kit::*;
use oraurust_core::{DbError, Page, ResultSet};

use crate::session::{Executed, Session};

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
    /// Bumped on every status change so observers can tell a re-run of the
    /// same statement from the previous result.
    generation: u64,
    /// True while a statement (or a page fetch) is in flight.
    busy: bool,
    cancel_requested: bool,
}

impl QueryRunner {
    pub fn new(session: Entity<Session>) -> Self {
        Self {
            session,
            status: RunStatus::Idle,
            generation: 0,
            busy: false,
            cancel_requested: false,
        }
    }

    pub fn status(&self) -> &RunStatus {
        &self.status
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
                    message: "Not connected. Connect to a profile first.".into(),
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
            this.update(cx, |this, cx| this.append(result, page, cx)).ok();
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
        let status = match outcome {
            Ok(Executed::Rows { page }) => {
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
            Ok(Executed::RowCount(affected)) => RunStatus::Dml(affected),
            Ok(Executed::Ddl) => RunStatus::Ddl,
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
                if cancelled {
                    if let RunStatus::Rows { result, capped, .. } = &self.status {
                        let kept = RunStatus::Rows {
                            result: result.clone(),
                            more: false,
                            capped: *capped,
                        };
                        self.set_status(kept, cx);
                        return;
                    }
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
