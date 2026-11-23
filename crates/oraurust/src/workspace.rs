//! The app shell workspace: a dock area with the PL/SQL editor pane in the
//! center, a left sidebar area for the future Schema Browser (issue #8),
//! and a bottom panel holding the Result Grid (issue #6).
//!
//! Ctrl+Enter in the editor runs the Statement under the cursor through
//! `QueryRunner` on the background executor (issue #6).

use gpui_kit::component::dock::{
    DockArea, DockLayout, DockPlacement, DockSkin, PanelEvent, panel_handle,
};
use gpui_kit::component::input::{Editor, EditorState, InputEvent, TabSize};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;
use oraurust_core::statement_at;

use crate::connection_panel::ConnectionPanel;
use crate::plsql;
use crate::result_grid::ResultGridPanel;
use crate::runner::QueryRunner;
use crate::session::Session;

const SAMPLE_PLSQL: &str = r#"-- oraurust sample (ticket #03 app shell)
CREATE OR REPLACE PACKAGE BODY emp_pkg AS

  g_min_salary CONSTANT NUMBER := 30000;
  g_dept_name  VARCHAR2(60) := 'Finance';

  PROCEDURE raise_salary(
    p_emp_id  IN employees.employee_id%TYPE,
    p_percent IN NUMBER DEFAULT 5
  ) IS
    v_current  NUMBER(10,2);
  BEGIN
    SELECT salary INTO v_current
      FROM employees
     WHERE employee_id = p_emp_id;

    IF v_current IS NULL THEN
      RAISE_APPLICATION_ERROR(-20001, 'No such employee');
    END IF;

    UPDATE employees
       SET salary = v_current * (1 + p_percent / 100)
     WHERE employee_id = p_emp_id;

    COMMIT;
  EXCEPTION
    WHEN NO_DATA_FOUND THEN
      NULL;
  END raise_salary;

END emp_pkg;
"#;

// ---------------------------------------------------------------------------
// Placeholder panels
// ---------------------------------------------------------------------------

/// A dock panel that only announces what will live there. Used for the
/// Schema Browser (issue #8) and Result Grid (issue #6) slots until those
/// tickets fill them in.
struct PlaceholderPanel {
    name: &'static str,
    title: &'static str,
    body: &'static str,
    focus_handle: FocusHandle,
}

impl PlaceholderPanel {
    fn new(
        name: &'static str,
        title: &'static str,
        body: &'static str,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            name,
            title,
            body,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for PlaceholderPanel {}
impl Focusable for PlaceholderPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PlaceholderPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().items_center().justify_center().child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(self.body),
        )
    }
}

impl gpui_kit::component::dock::BasePanel for PlaceholderPanel {
    fn panel_name(&self) -> &'static str {
        self.name
    }
}

impl gpui_kit::component::dock::Panel for PlaceholderPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.title.into_any_element()
    }
}

// ---------------------------------------------------------------------------
// Editor panel
// ---------------------------------------------------------------------------

/// The PL/SQL editor pane in the dock center.
struct EditorPanel {
    editor: Entity<EditorState>,
    runner: Entity<QueryRunner>,
    focus_handle: FocusHandle,
    _observed_editor: Subscription,
}

impl EditorPanel {
    fn new(
        runner: Entity<QueryRunner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(plsql::LANG_NAME)
                .line_number(true)
                .indent_guides(true)
                .tab_size(TabSize {
                    tab_size: 4,
                    hard_tabs: false,
                })
                .soft_wrap(false)
                .default_value(SAMPLE_PLSQL)
        });
        let observed_editor = cx.subscribe_in(
            &editor,
            window,
            |this, editor, event: &InputEvent, _window, cx| {
                // Ctrl+Enter arrives as a secondary Enter; the editor never
                // inserts a newline for it (issue #6).
                if matches!(
                    event,
                    InputEvent::PressEnter {
                        secondary: true,
                        ..
                    }
                ) {
                    this.run_under_cursor(editor, cx);
                }
            },
        );
        Self {
            editor,
            runner,
            focus_handle: cx.focus_handle(),
            _observed_editor: observed_editor,
        }
    }

    /// Runs the Statement containing the cursor (issue #6). The split is
    /// pure and runs on the UI thread; the Oracle call does not.
    fn run_under_cursor(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let (text, cursor) = {
            let state = editor.read(cx);
            (state.value().to_string(), state.cursor())
        };
        let Some(statement) = statement_at(&text, cursor) else {
            return;
        };
        self.runner
            .update(cx, |runner, cx| runner.run(statement.text, cx));
    }
}

impl EventEmitter<PanelEvent> for EditorPanel {}
impl Focusable for EditorPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.clone();
        div()
            .id("editor-panel")
            .size_full()
            .track_focus(&self.focus_handle)
            .child(
                Editor::new(&editor)
                    .bordered(false)
                    .h_full()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size),
            )
    }
}

impl gpui_kit::component::dock::BasePanel for EditorPanel {
    fn panel_name(&self) -> &'static str {
        "EditorPanel"
    }
}

impl gpui_kit::component::dock::Panel for EditorPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "PL/SQL Editor".into_any_element()
    }
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

pub struct Workspace {
    dock_area: Entity<DockArea>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (dock_area, _skin) = DockSkin::dock_area("oraurust-dock", Some(1), window, cx);
        let session = cx.new(|_| Session::default());
        let runner = cx.new(|_| QueryRunner::new(session.clone()));
        let center = DockLayout::tabs().panel_view(
            panel_handle(cx.new(|cx| EditorPanel::new(runner.clone(), window, cx))),
            cx,
        );
        let left = DockLayout::tabs().panel_view(
            panel_handle(cx.new(|cx| {
                PlaceholderPanel::new(
                    "Schema Browser",
                    "Schema Browser",
                    "Schema Browser — coming in ticket 06 (issue #8)",
                    cx,
                )
            })),
            cx,
        );
        let right = DockLayout::tabs().panel_view(
            panel_handle(cx.new(|cx| ConnectionPanel::new(session, window, cx))),
            cx,
        );
        let bottom = DockLayout::tabs().panel_view(
            panel_handle(cx.new(|cx| ResultGridPanel::new(runner, window, cx))),
            cx,
        );

        dock_area.update(cx, |view, cx| {
            view.set_center(center, window, cx);
            view.set_dock(DockPlacement::Left, left, window, cx);
            view.set_dock_size(DockPlacement::Left, px(220.), window, cx);
            view.set_dock_collapsible(DockPlacement::Left, true, window, cx);
            view.set_dock(DockPlacement::Right, right, window, cx);
            view.set_dock_size(DockPlacement::Right, px(280.), window, cx);
            view.set_dock_collapsible(DockPlacement::Right, true, window, cx);
            view.set_dock(DockPlacement::Bottom, bottom, window, cx);
            view.set_dock_size(DockPlacement::Bottom, px(220.), window, cx);
            view.set_dock_collapsible(DockPlacement::Bottom, true, window, cx);
        });

        Self { dock_area }
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("workspace")
            .size_full()
            .child(self.dock_area.clone())
    }
}
