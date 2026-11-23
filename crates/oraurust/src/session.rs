//! The live Connection shared by the panels (issue #6).
//!
//! `ConnectionPanel` opens the session; the Editor's Ctrl+Enter execution and
//! the Result Grid read it through this entity. It holds an `Arc` so a
//! statement can run on the background executor while the UI keeps a handle
//! to cancel it (issue #6's cancel control).

use std::sync::Arc;

use gpui_kit::*;
use oraurust_core::Connection;

/// The live session, shared by the panels (issue #6).
///
/// The connection is a `dyn Connection`, not the concrete ODPI-C type, so the
/// app keeps talking to the seam ADR-0001 defines — and the background
/// executor only needs `Send + Sync`, which the trait promises.
pub type SharedConnection = Arc<dyn Connection>;

/// The session state, as shown in the Connection panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    Disconnected,
    Connecting(String),
    Connected { profile: String, detail: String },
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

/// The app's current Oracle session, if any.
#[derive(Default)]
pub struct Session {
    state: SessionState,
    connection: Option<SharedConnection>,
}

impl Session {
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// The live connection, when connected. Cloning the `Arc` lets a
    /// background task execute while the UI keeps one to cancel.
    pub fn connection(&self) -> Option<SharedConnection> {
        self.connection.clone()
    }

    pub fn set_connecting(&mut self, profile: &str, cx: &mut Context<Self>) {
        self.state = SessionState::Connecting(profile.to_string());
        self.connection = None;
        cx.notify();
    }

    pub fn set_connected(
        &mut self,
        profile: &str,
        detail: String,
        connection: SharedConnection,
        cx: &mut Context<Self>,
    ) {
        self.state = SessionState::Connected {
            profile: profile.to_string(),
            detail,
        };
        self.connection = Some(connection);
        cx.notify();
    }

    pub fn set_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.state = SessionState::Failed(message);
        self.connection = None;
        cx.notify();
    }

    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.state = SessionState::Disconnected;
        self.connection = None;
        cx.notify();
    }
}
