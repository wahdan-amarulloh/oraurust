//! The live Connection and its worker thread (issue #6, ADR-0002).
//!
//! `ConnectionPanel` opens the session; the Editor's Ctrl+Enter execution and
//! the Result Grid reach the database only through [`SessionWorker`]. The
//! worker owns the connection and the open cursor on its own thread, so OCI
//! calls are serialised and the GPUI thread never blocks. Cancel bypasses the
//! worker's queue — the worker is blocked in `execute` — and goes through a
//! [`CancelHandle`] over the same session (ADR-0002).

use std::sync::Arc;

use gpui_kit::*;
use oraurust_core::{
    CancelHandle, Connection, Cursor, DbError, FETCH_ROWS, Page, Result, StatementOutcome,
    TransactionEffect, TransactionEnd,
};

use crate::oracle_conn::OraConnection;

/// Commands queued ahead of the worker. Small: the UI awaits each command's
/// reply before sending the next, so this is only slack, and a bounded queue
/// gives backpressure rather than unbounded growth (ADR-0002).
const COMMAND_QUEUE_DEPTH: usize = 8;

/// The session state, as shown in the Connection panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    Disconnected,
    Connecting(String),
    Connected {
        profile: String,
        detail: String,
    },
    Failed(String),
}

impl SessionState {
    pub fn is_connected(&self) -> bool {
        matches!(self, SessionState::Connected { .. })
    }

    pub fn is_connecting(&self) -> bool {
        matches!(self, SessionState::Connecting(_))
    }

    /// The name of the profile this session is connected to, if any.
    pub fn connected_profile(&self) -> Option<&str> {
        match self {
            SessionState::Connected { profile, .. } => Some(profile),
            _ => None,
        }
    }
}

/// What one `Execute` produced, as the worker reports it.
///
/// The worker keeps the cursor; the UI only needs the first page and whether
/// more rows remain, so this is not a [`StatementOutcome`] (which carries the
/// cursor the worker owns). The [`TransactionEffect`] travels alongside so the
/// app never re-derives the dirty rule the core owns (issue #7).
#[derive(Debug)]
pub struct Executed {
    pub outcome: ExecutedOutcome,
    pub effect: TransactionEffect,
}

/// The UI-facing shape of a [`StatementOutcome`].
#[derive(Debug)]
pub enum ExecutedOutcome {
    Rows {
        page: Page,
    },
    RowCount(u64),
    Ddl,
    /// COMMIT or ROLLBACK succeeded: the session's Transaction ended.
    Transaction(TransactionEnd),
}

/// What the UI asks the worker thread to do.
enum Command {
    Execute {
        statement: String,
        reply: flume::Sender<Result<Executed>>,
    },
    Fetch {
        reply: flume::Sender<Result<Page>>,
    },
}

/// A handle to the session's worker thread.
///
/// `Clone` so a background task can execute while the UI keeps one to cancel.
#[derive(Clone)]
pub struct SessionWorker {
    commands: flume::Sender<Command>,
    cancel: Arc<dyn CancelHandle>,
}

impl SessionWorker {
    /// Runs `statement` on the worker thread. A SELECT's first page comes
    /// back with `more` set when further pages remain; the worker keeps the
    /// cursor.
    pub async fn execute(&self, statement: String) -> Result<Executed> {
        let (reply, response) = flume::bounded(1);
        if self
            .commands
            .send_async(Command::Execute { statement, reply })
            .await
            .is_err()
        {
            return Err(worker_stopped());
        }
        response
            .recv_async()
            .await
            .unwrap_or_else(|_| Err(worker_stopped()))
    }

    /// Fetches another page of the open Result Set.
    pub async fn fetch_more(&self) -> Result<Page> {
        let (reply, response) = flume::bounded(1);
        if self
            .commands
            .send_async(Command::Fetch { reply })
            .await
            .is_err()
        {
            return Err(worker_stopped());
        }
        response
            .recv_async()
            .await
            .unwrap_or_else(|_| Err(worker_stopped()))
    }

    /// Breaks the running statement through the cancel handle, not the
    /// worker's queue, which is blocked while `execute` runs (ADR-0002).
    pub fn cancel(&self) -> Result<()> {
        self.cancel.cancel()
    }
}

/// The worker loop: owns the connection and the open cursor, one command at
/// a time.
fn worker_loop(connection: Box<dyn Connection>, commands: flume::Receiver<Command>) {
    let mut cursor: Option<Box<dyn Cursor>> = None;
    // `recv()` returns `Err` when every sender is dropped (disconnect).
    while let Ok(command) = commands.recv() {
        match command {
            Command::Execute { statement, reply } => {
                // A new statement abandons any previous Result Set.
                cursor = None;
                let executed = match connection.execute(&statement) {
                    Ok(outcome) => {
                        let effect = TransactionEffect::of_outcome(&outcome);
                        let outcome = match outcome {
                            StatementOutcome::Rows { page, cursor: next } => {
                                cursor = next;
                                ExecutedOutcome::Rows { page }
                            }
                            StatementOutcome::RowCount(count) => ExecutedOutcome::RowCount(count),
                            StatementOutcome::Ddl => ExecutedOutcome::Ddl,
                            StatementOutcome::Transaction(end) => ExecutedOutcome::Transaction(end),
                        };
                        Ok(Executed { outcome, effect })
                    }
                    Err(error) => Err(error),
                };
                let _ = reply.send(executed);
            }
            Command::Fetch { reply } => {
                let result = match cursor.as_mut() {
                    Some(cursor) => cursor.fetch(FETCH_ROWS),
                    None => Err(DbError::new("", "no result set is open", None)),
                };
                // Drop an exhausted cursor so its server-side resources free
                // up, and an errored one rather than reuse a broken handle.
                let keep = result.as_ref().is_ok_and(|page| page.more);
                if !keep {
                    cursor = None;
                }
                let _ = reply.send(result);
            }
        }
    }
}

fn worker_stopped() -> DbError {
    DbError::new("", "the session worker stopped", None)
}

/// The app's current Oracle session, if any.
#[derive(Default)]
pub struct Session {
    state: SessionState,
    worker: Option<SessionWorker>,
}

impl Session {
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// The session's worker handle, when connected.
    pub fn worker(&self) -> Option<SessionWorker> {
        self.worker.clone()
    }

    pub fn set_connecting(&mut self, profile: &str, cx: &mut Context<Self>) {
        self.state = SessionState::Connecting(profile.to_string());
        self.worker = None;
        cx.notify();
    }

    /// Takes `connection` over and starts its worker thread.
    ///
    /// The connection enters the worker as the [`Connection`] seam — the app
    /// never touches the driver directly (ADR-0001); only the cancel handle
    /// comes from the concrete type (ADR-0002).
    pub fn set_connected(
        &mut self,
        profile: &str,
        detail: String,
        connection: OraConnection,
        cx: &mut Context<Self>,
    ) {
        let cancel = Arc::from(connection.cancel_handle());
        let (commands, receiver) = flume::bounded(COMMAND_QUEUE_DEPTH);
        std::thread::Builder::new()
            .name("oraurust-session".into())
            .spawn(move || worker_loop(Box::new(connection), receiver))
            .expect("spawn the session worker thread");
        self.worker = Some(SessionWorker { commands, cancel });
        self.state = SessionState::Connected {
            profile: profile.to_string(),
            detail,
        };
        cx.notify();
    }

    pub fn set_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.state = SessionState::Failed(message);
        self.worker = None;
        cx.notify();
    }

    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.state = SessionState::Disconnected;
        self.worker = None;
        cx.notify();
    }
}
