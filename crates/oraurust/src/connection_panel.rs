//! Connection panel (issue #5): create, edit, delete and persist named
//! profiles, and connect/disconnect against a real Oracle session.
//!
//! All database work runs on the background executor (ADR-0001): the GPUI
//! thread never blocks on Oracle. Profiles are persisted through
//! `ProfileStore`, which seals passwords with DPAPI.

use std::path::PathBuf;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use oraurust_core::instant_client::{ClientStatus, check, remediation};
use oraurust_core::{Profile, ProfileStore};

use crate::oracle_conn::OraConnection;
use crate::session::{Session, SessionState};

/// Where profiles are persisted: `%APPDATA%\oraurust\profiles.json`.
fn default_store() -> ProfileStore {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    ProfileStore::new(base.join("oraurust").join("profiles.json"))
}

/// Directories probed for `oci.dll` by the startup Instant Client check.
fn client_search_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("ORACLE_HOME") {
        paths.push(PathBuf::from(&home).join("bin"));
        paths.push(PathBuf::from(home));
    }
    paths
}

/// A dock panel for managing Connection profiles.
pub struct ConnectionPanel {
    store: ProfileStore,
    profiles: Vec<Profile>,
    selected: Option<usize>,
    name: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    service: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    /// The live session, shared with the Editor's execution and the Result
    /// Grid (issue #6).
    session: Entity<Session>,
    _observed_session: Subscription,
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for ConnectionPanel {}
impl Focusable for ConnectionPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ConnectionPanel {
    pub fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut text_input = |placeholder: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder.to_string())
                    .masked(false)
            })
        };
        let name = text_input("Profile name", cx);
        let host = text_input("Host, e.g. db.example.com", cx);
        let port = text_input("Port (1521)", cx);
        let service = text_input("Service name or SID", cx);
        let username = text_input("Username", cx);
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password".to_string())
                .masked(true)
        });
        let observed_session = cx.observe(&session, |_this, _session, cx| cx.notify());

        let mut panel = Self {
            store: default_store(),
            profiles: Vec::new(),
            selected: None,
            name,
            host,
            port,
            service,
            username,
            password,
            session,
            _observed_session: observed_session,
            focus_handle: cx.focus_handle(),
        };
        panel.reload(cx);
        panel
    }

    fn state(&self, cx: &App) -> SessionState {
        self.session.read(cx).state().clone()
    }

    /// Re-reads profiles from disk; also surfaces a startup Instant Client
    /// problem in the status line (issue #5: readable error, not a panic).
    fn reload(&mut self, cx: &mut Context<Self>) {
        if let ClientStatus::Missing = check(&client_search_paths()) {
            self.fail(remediation(), cx);
        }
        match self.store.load() {
            Ok(profiles) => self.profiles = profiles,
            Err(e) => self.fail(e.to_string(), cx),
        }
        if self.selected.is_some_and(|ix| ix >= self.profiles.len()) {
            self.selected = None;
        }
        cx.notify();
    }

    /// Marks the session failed and shows `message` in the status line.
    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.set_failed(message, cx));
    }

    fn value(&self, input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().to_string()
    }

    /// Builds a Profile from the form; `None` when required fields are empty
    /// or the port is not a number.
    fn profile_from_form(&self, cx: &App) -> Option<Profile> {
        let name = self.value(&self.name, cx).trim().to_string();
        let host = self.value(&self.host, cx).trim().to_string();
        let service = self.value(&self.service, cx).trim().to_string();
        let username = self.value(&self.username, cx).trim().to_string();
        let port_text = self.value(&self.port, cx);
        let port: u16 = if port_text.trim().is_empty() {
            1521
        } else {
            port_text.trim().parse().ok()?
        };
        if name.is_empty() || host.is_empty() || service.is_empty() || username.is_empty() {
            return None;
        }
        let mut profile = Profile::new(name, host, port, service, username);
        profile.password = self.value(&self.password, cx);
        Some(profile)
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.profile_from_form(cx) else {
            window.push_notification(
                Notification::error(
                    "Fill in name, host, service/SID and username (port must be a number).",
                ),
                cx,
            );
            return;
        };
        match self.store.save(&profile) {
            Ok(()) => {
                window.push_notification(
                    Notification::success(format!("Saved profile {}", profile.name)),
                    cx,
                );
            }
            Err(e) => {
                window.push_notification(Notification::error(e.to_string()), cx);
            }
        }
        self.reload(cx);
    }

    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.selected_profile().cloned() else {
            return;
        };
        if let Err(e) = self.store.delete(&profile.name) {
            window.push_notification(Notification::error(e.to_string()), cx);
        }
        if self.state(cx).connected_profile() == Some(profile.name.as_str()) {
            self.session
                .update(cx, |session, cx| session.disconnect(cx));
        }
        self.selected = None;
        self.reload(cx);
    }

    fn selected_profile(&self) -> Option<&Profile> {
        self.profiles.get(self.selected?)
    }

    /// Connects to the selected profile on the background executor.
    fn connect_selected(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.selected_profile().cloned() else {
            return;
        };
        if self.state(cx).is_connecting() {
            return;
        }
        self.session
            .update(cx, |session, cx| session.set_connecting(&profile.name, cx));

        cx.spawn(async move |this, cx| {
            // ODPI-C is blocking: run the whole handshake off the UI thread.
            let result = cx
                .background_spawn(async move {
                    OraConnection::connect(&profile).map(|session| (session, profile))
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((connection, profile)) => {
                    let detail = format!("{}@{}", profile.username, profile.connect_string());
                    this.session.update(cx, |session, cx| {
                        session.set_connected(&profile.name, detail, connection, cx)
                    });
                }
                Err(e) => {
                    this.session
                        .update(cx, |session, cx| session.set_failed(e.to_string(), cx));
                }
            })
            .ok();
        })
        .detach();
    }

    fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.disconnect(cx));
    }

    fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        let Some(profile) = self.profiles.get(ix).cloned() else {
            return;
        };
        self.name
            .update(cx, |s, cx| s.set_value(profile.name, window, cx));
        self.host
            .update(cx, |s, cx| s.set_value(profile.host, window, cx));
        self.port.update(cx, |s, cx| {
            s.set_value(profile.port.to_string(), window, cx)
        });
        self.service
            .update(cx, |s, cx| s.set_value(profile.service, window, cx));
        self.username
            .update(cx, |s, cx| s.set_value(profile.username, window, cx));
        // The stored password was unsealed by the store; prefill it so a
        // "Save" after editing another field keeps the password.
        self.password
            .update(cx, |s, cx| s.set_value(profile.password, window, cx));
        cx.notify();
    }

    fn field(&self, label: &str, input: &Entity<InputState>, cx: &App) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.to_string()),
            )
            .child(Input::new(input))
    }
}

impl BasePanel for ConnectionPanel {
    fn panel_name(&self) -> &'static str {
        "ConnectionPanel"
    }
}

impl Panel for ConnectionPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Connections".into_any_element()
    }
}

impl Render for ConnectionPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state(cx);
        let (status, status_color) = match &state {
            SessionState::Disconnected => {
                ("Not connected".to_string(), cx.theme().muted_foreground)
            }
            SessionState::Connecting(name) => (
                format!("Connecting to {name}…"),
                cx.theme().muted_foreground,
            ),
            SessionState::Connected { profile, detail } => (
                format!("Connected: {profile} ({detail})"),
                cx.theme().success,
            ),
            SessionState::Failed(msg) => (msg.clone(), cx.theme().danger),
        };
        let is_connected = state.is_connected();
        let connecting = state.is_connecting();
        let has_selection = self.selected.is_some();

        let mut list = v_flex().gap_1();
        for (ix, profile) in self.profiles.iter().enumerate() {
            let active = self.selected == Some(ix);
            list = list.child(
                div()
                    .id(("profile", ix))
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .cursor_pointer()
                    .when(active, |this| this.bg(cx.theme().accent))
                    .hover(|this| this.bg(cx.theme().button_hover))
                    .child(
                        div()
                            .text_sm()
                            .child(format!("{} — {}", profile.name, profile.username)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(profile.connect_string()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select(ix, window, cx);
                    })),
            );
        }

        v_flex()
            .size_full()
            .p_3()
            .gap_3()
            .overflow_hidden()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("connect").label("Connect").on_click(
                            cx.listener(|this, _, _window, cx| this.connect_selected(cx)),
                        ),
                    )
                    .child(
                        Button::new("disconnect")
                            .label("Disconnect")
                            .when(!is_connected, |b| b.disabled(true))
                            .on_click(cx.listener(|this, _, _window, cx| this.disconnect(cx))),
                    )
                    .child(
                        Button::new("delete")
                            .label("Delete")
                            .when(!has_selection, |b| b.disabled(true))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.delete_selected(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .when(connecting, |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    .text_color(status_color)
                    .child(status),
            )
            .child(
                div()
                    .text_xs()
                    .child("Saved profiles")
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().flex_1().overflow_hidden().child(list))
            .child(div().h(px(1.)).bg(cx.theme().border))
            .child(self.field("Name", &self.name.clone(), cx))
            .child(self.field("Host", &self.host.clone(), cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(self.field("Port", &self.port.clone(), cx)),
                    )
                    .child(v_flex().flex_1().child(self.field(
                        "Service / SID",
                        &self.service.clone(),
                        cx,
                    ))),
            )
            .child(self.field("Username", &self.username.clone(), cx))
            .child(self.field("Password", &self.password.clone(), cx))
            .child(
                Button::new("save")
                    .label("Save profile")
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
    }
}
