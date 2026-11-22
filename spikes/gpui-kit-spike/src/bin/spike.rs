//! Spike window: gpui-kit dock with a PL/SQL-highlighted code editor panel
//! and a 10 000-row virtualized table panel, on Windows.

use gpui_kit::assets::Assets;
use gpui_kit::component::dock::{
    DockArea, DockLayout, DockPlacement, DockSkin, PanelEvent, panel_handle,
};
use gpui_kit::component::highlighter::LanguageRegistry;
use gpui_kit::component::input::{Editor, EditorState, TabSize};
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

actions!(spike, [Quit]);

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

/// The PL/SQL editor panel.
struct EditorPanel {
    editor: Entity<EditorState>,
    focus_handle: FocusHandle,
}

impl EditorPanel {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(gpui_kit_spike::plsql::LANG_NAME)
                .line_number(true)
                .indent_guides(true)
                .tab_size(TabSize {
                    tab_size: 4,
                    hard_tabs: false,
                })
                .soft_wrap(false)
                .default_value(SAMPLE_PLSQL)
        });
        Self {
            editor,
            focus_handle: cx.focus_handle(),
        }
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
// The 10k-row result table
// ---------------------------------------------------------------------------

/// One fake "result set" row, mimicking a SELECT over employees.
struct Row {
    id: u32,
    name: SharedString,
    email: SharedString,
    salary: f64,
    department: SharedString,
}

fn make_rows(n: usize) -> Vec<Row> {
    let first = ["ALICE", "BOB", "CAROL", "DAVE", "EVE", "FRANK", "GRACE", "HEIDI"];
    let last = ["SMITH", "JONES", "BROWN", "WILSON", "MOORE", "TAYLOR", "DAVIS"];
    let depts = ["ACCOUNTING", "RESEARCH", "SALES", "OPERATIONS", "FINANCE"];

    (0..n)
        .map(|i| {
            let f = first[i % first.len()];
            let l = last[(i / first.len()) % last.len()];
            Row {
                id: 1000 + i as u32,
                name: format!("{f} {l}").into(),
                email: format!("{}{}{}@corp.example", f[..1].to_lowercase(), l.to_lowercase(), i)
                    .into(),
                salary: 30_000.0 + (i as f64 * 137.0) % 70_000.0,
                department: depts[i % depts.len()].into(),
            }
        })
        .collect()
}

struct ResultGridDelegate {
    rows: Vec<Row>,
    columns: Vec<Column>,
}

impl ResultGridDelegate {
    fn new(n_rows: usize) -> Self {
        Self {
            rows: make_rows(n_rows),
            columns: vec![
                Column::new("id", "ID").width(px(80.)),
                Column::new("name", "NAME").width(px(160.)),
                Column::new("email", "EMAIL").width(px(240.)),
                Column::new("salary", "SALARY").width(px(120.)),
                Column::new("department", "DEPARTMENT").width(px(160.)),
            ],
        }
    }
}

impl TableDelegate for ResultGridDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, _: &App) -> Option<SharedString> {
        let row = self.rows.get(row_ix)?;
        Some(match col_ix {
            0 => row.id.to_string(),
            1 => row.name.to_string(),
            2 => row.email.to_string(),
            3 => format!("{:.2}", row.salary),
            4 => row.department.to_string(),
            _ => return None,
        }
        .into())
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text = self.cell_label(row_ix, col_ix, cx).unwrap_or_default();
        div()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .px_2()
            .overflow_hidden()
            .child(text)
    }
}

/// The Result Grid panel.
struct GridPanel {
    table: Entity<TableState<ResultGridDelegate>>,
    focus_handle: FocusHandle,
}

impl GridPanel {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = ResultGridDelegate::new(10_000);
        let table = cx.new(|cx| TableState::new(delegate, window, cx));
        Self {
            table,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for GridPanel {}
impl Focusable for GridPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GridPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(DataTable::new(&self.table).stripe(true))
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} rows fetched",
                        self.table.read(cx).delegate().rows.len()
                    )),
            )
    }
}

impl gpui_kit::component::dock::BasePanel for GridPanel {
    fn panel_name(&self) -> &'static str {
        "GridPanel"
    }
}

impl gpui_kit::component::dock::Panel for GridPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Result Grid (10k)".into_any_element()
    }
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

struct Workspace {
    dock_area: Entity<DockArea>,
}

impl Workspace {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (dock_area, _skin) = DockSkin::dock_area("spike-dock", Some(1), window, cx);

        Self::reset_default_layout(&dock_area, window, cx);

        Self { dock_area }
    }

    fn reset_default_layout(dock_area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
        let center = DockLayout::tabs()
            .panel_view(panel_handle(cx.new(|cx| EditorPanel::new(window, cx))), cx);
        let bottom = DockLayout::tabs()
            .panel_view(panel_handle(cx.new(|cx| GridPanel::new(window, cx))), cx);

        _ = dock_area.update(cx, |view, cx| {
            view.set_center(center, window, cx);
            view.set_dock(DockPlacement::Bottom, bottom, window, cx);
            view.set_dock_size(DockPlacement::Bottom, px(260.), window, cx);
            view.set_dock_collapsible(DockPlacement::Bottom, true, window, cx);
        });
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

// ---------------------------------------------------------------------------
// Sample
// ---------------------------------------------------------------------------

const SAMPLE_PLSQL: &str = r#"-- Employee salary report (spike sample)
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

    /* multi-line comment
       covering the commit */
    COMMIT;
  EXCEPTION
    WHEN NO_DATA_FOUND THEN
      NULL;
  END raise_salary;

END emp_pkg;
"#;

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    // Register PL/SQL with the highlighter before any editor renders.
    gpui_kit_spike::plsql::register();

    let app = gpui_kit::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        cx.activate(true);
        println!("spike window opening");

        cx.bind_keys(vec![]);
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());

        // Verify the query actually compiled at startup; a broken query would
        // otherwise surface only as a tracing error and silently-grey editor.
        {
            let registry = LanguageRegistry::singleton();
            let config = registry
                .language(gpui_kit_spike::plsql::LANG_NAME)
                .expect("plsql config registered");
            let _ = tree_sitter::Query::new(&gpui_kit_spike::plsql::language(), &config.highlights)
                .expect("plsql highlights query failed to compile");
        }

        let window_size = cx
            .primary_display()
            .map(|d| {
                let s = d.bounds().size;
                size(
                    (s.width * 0.85).min(px(1500.)),
                    (s.height * 0.85).min(px(1000.)),
                )
            })
            .unwrap_or(size(px(1280.), px(800.)));

        let _ = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None, window_size, cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("oraurust spike".into()),
                    appears_transparent: false,
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        )
        .expect("failed to open spike window");
    });

    println!("spike window open");
}
