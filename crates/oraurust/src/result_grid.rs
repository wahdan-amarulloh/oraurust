//! The Result Grid (issues #6 and #7): a virtualized table over the last
//! SELECT's Result Set, the status line for every execution (row count, DML
//! affected rows, DDL, or a readable Oracle error), the session's Transaction
//! state with Commit/Rollback, and CSV export of the grid.
//!
//! The grid renders from `QueryRunner`'s status only; all fetching happened on
//! the background executor. Stripe rendering makes odd and even records
//! slightly different colors (ticket #6's acceptance criterion).

use std::sync::Arc;

use gpui_kit::component::button::Button;
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::table::{Column as TableColumn, DataTable, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use oraurust_core::{ROW_CAP, ResultSet, TransactionEnd, to_csv};

use crate::runner::{QueryRunner, RunStatus};

/// Width a grid column gets before it is resized by hand.
const DEFAULT_COLUMN_WIDTH: f32 = 140.;

/// The Result Grid's table delegate: column metadata + fetched rows.
///
/// Rows are shared with the runner's status (`Arc`), so a large Result Set is
/// not copied when the grid appears.
struct ResultGridDelegate {
    columns: Vec<TableColumn>,
    result: Arc<ResultSet>,
}

impl ResultGridDelegate {
    fn from_result_set(result: Arc<ResultSet>) -> Self {
        let columns = result
            .columns
            .iter()
            .enumerate()
            .map(|(ix, column)| {
                // The type rides along in the header, as ticket #6 asks for
                // "column names + types".
                TableColumn::new(
                    ix.to_string(),
                    format!("{} : {}", column.name, column.type_name),
                )
                .width(px(DEFAULT_COLUMN_WIDTH))
            })
            .collect();
        Self { columns, result }
    }

    /// The cell's value; `None` is SQL NULL.
    fn cell(&self, row_ix: usize, col_ix: usize) -> Option<Option<&str>> {
        let row = self.result.rows.get(row_ix)?;
        row.get(col_ix).map(|value| value.as_deref())
    }
}

impl TableDelegate for ResultGridDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.result.rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> TableColumn {
        self.columns[col_ix].clone()
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, _: &App) -> Option<SharedString> {
        self.cell(row_ix, col_ix)
            .map(|value| value.unwrap_or("NULL").into())
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let value = self.cell(row_ix, col_ix).flatten();
        let is_null = value.is_none();
        div()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .px_2()
            .overflow_hidden()
            .when(is_null, |this| this.text_color(cx.theme().muted_foreground))
            .child(value.unwrap_or("NULL").to_string())
    }
}

/// The dock panel that shows execution output.
pub struct ResultGridPanel {
    runner: Entity<QueryRunner>,
    table: Option<Entity<TableState<ResultGridDelegate>>>,
    /// Which `QueryRunner` generation `table` was built from.
    table_generation: u64,
    focus_handle: FocusHandle,
    _observed_runner: Subscription,
}

/// File name suggested in the CSV save dialog.
const CSV_FILE_NAME: &str = "result.csv";

impl ResultGridPanel {
    pub fn new(runner: Entity<QueryRunner>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observed_runner = cx.observe_in(&runner, window, |this, runner, window, cx| {
            let generation = runner.read(cx).generation();
            this.sync_table(generation, window, cx);
            cx.notify();
        });
        let mut panel = Self {
            runner,
            table: None,
            table_generation: u64::MAX,
            focus_handle: cx.focus_handle(),
            _observed_runner: observed_runner,
        };
        let generation = panel.runner.read(cx).generation();
        panel.sync_table(generation, window, cx);
        panel
    }

    /// Rebuilds the table when the runner's result changed, so it is not
    /// recreated on unrelated redraws.
    fn sync_table(&mut self, generation: u64, window: &mut Window, cx: &mut Context<Self>) {
        if generation == self.table_generation {
            return;
        }
        self.table_generation = generation;
        self.table = match self.runner.read(cx).status() {
            RunStatus::Rows { result, .. } => {
                let delegate = ResultGridDelegate::from_result_set(result.clone());
                Some(cx.new(|cx| TableState::new(delegate, window, cx).cell_selectable(false)))
            }
            _ => None,
        };
    }

    /// Prompts for a file and writes the current Result Set to it as CSV
    /// (issue #7). The write runs on the background executor; the UI thread
    /// only formats the text, which is bounded by the row cap.
    fn export_csv(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let RunStatus::Rows { result, .. } = self.runner.read(cx).status() else {
            window.push_notification(Notification::warning("No result set to export."), cx);
            return;
        };
        let result = result.clone();
        let initial_directory = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        cx.spawn_in(window, async move |_this, cx| {
            let prompt = cx.update(|_window, app| {
                app.prompt_for_new_path(&initial_directory, Some(CSV_FILE_NAME))
            });
            let Ok(prompt) = prompt else {
                return;
            };
            let Ok(Ok(Some(path))) = prompt.await else {
                // Cancelled, or the platform has no file picker: no export.
                return;
            };
            let csv = to_csv(&result);
            let target = path.clone();
            let written = cx
                .background_spawn(async move { std::fs::write(&target, csv) })
                .await;
            cx.update(|window, app| match written {
                Ok(()) => window.push_notification(
                    Notification::success(format!("Exported {}", path.display())),
                    app,
                ),
                Err(error) => window
                    .push_notification(Notification::error(format!("Export failed: {error}")), app),
            })
            .ok();
        })
        .detach();
    }
}

impl EventEmitter<PanelEvent> for ResultGridPanel {}
impl Focusable for ResultGridPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ResultGridPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let runner = self.runner.read(cx);
        let status = runner.status().clone();
        let busy = runner.is_busy();
        let transaction = runner.transaction();
        let commit_error = runner.end_error().cloned();
        // Export only a finished Result Set: while pages are still being
        // fetched `busy` is set, and exporting now would silently write a
        // partial grid.
        let can_export = matches!(&status, RunStatus::Rows { .. }) && !busy;

        let (line, color) = match &status {
            RunStatus::Idle => (
                "Run the statement under the cursor with Ctrl+Enter.".to_string(),
                cx.theme().muted_foreground,
            ),
            RunStatus::Running => ("Running...".to_string(), cx.theme().muted_foreground),
            RunStatus::Rows {
                result,
                more,
                capped,
            } => {
                let fetched = result.rows.len();
                let suffix = if *capped {
                    format!(" (row cap {ROW_CAP} reached; more rows not fetched)")
                } else if *more {
                    " (fetching more…)".to_string()
                } else {
                    String::new()
                };
                (
                    format!(
                        "{} row{} fetched{suffix}",
                        fetched,
                        if fetched == 1 { "" } else { "s" }
                    ),
                    cx.theme().muted_foreground,
                )
            }
            RunStatus::Dml(affected) => (
                format!(
                    "{affected} row{} affected (uncommitted)",
                    if *affected == 1 { "" } else { "s" }
                ),
                cx.theme().success,
            ),
            RunStatus::Ddl => ("Done (implicit commit).".to_string(), cx.theme().success),
            RunStatus::Transaction(end) => (end.label().to_string(), cx.theme().success),
            RunStatus::Failed {
                message, cancelled, ..
            } => (
                if *cancelled {
                    format!("Cancelled: {message}")
                } else {
                    message.clone()
                },
                cx.theme().danger,
            ),
        };

        let body: AnyElement = match &self.table {
            Some(table) => DataTable::new(table).stripe(true).into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("No result set")
                .into_any_element(),
        };

        // Commit/Rollback stay available whenever the session has uncommitted
        // changes, even while a SELECT's grid is showing (issue #7: they
        // control the session's transaction, not the last statement's).
        let can_end_transaction = transaction.is_dirty() && !busy;

        // The Transaction indicator: a failed Commit/Rollback is reported here
        // instead of clearing the grid, because the transaction is still open.
        let (transaction_line, transaction_color) = match &commit_error {
            Some(error) => (
                format!("Transaction still open: {error}"),
                cx.theme().danger,
            ),
            None if transaction.is_dirty() => (transaction.label().to_string(), cx.theme().warning),
            None => (transaction.label().to_string(), cx.theme().muted_foreground),
        };

        v_flex()
            .size_full()
            .child(div().flex_1().overflow_hidden().child(body))
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("cancel-statement")
                            .label("Cancel")
                            .when(!busy, |button| button.disabled(true))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.runner.update(cx, |runner, cx| runner.cancel(cx));
                            })),
                    )
                    .child(
                        Button::new("commit")
                            .label("Commit")
                            .when(!can_end_transaction, |button| button.disabled(true))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.runner.update(cx, |runner, cx| {
                                    runner.end_transaction(TransactionEnd::Commit, cx)
                                });
                            })),
                    )
                    .child(
                        Button::new("rollback")
                            .label("Rollback")
                            .when(!can_end_transaction, |button| button.disabled(true))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.runner.update(cx, |runner, cx| {
                                    runner.end_transaction(TransactionEnd::Rollback, cx)
                                });
                            })),
                    )
                    .child(
                        Button::new("export-csv")
                            .label("Export CSV")
                            .when(!can_export, |button| button.disabled(true))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.export_csv(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .overflow_hidden()
                            .child(line)
                            .text_color(color),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .overflow_hidden()
                            .child(transaction_line)
                            .text_color(transaction_color),
                    ),
            )
    }
}

impl BasePanel for ResultGridPanel {
    fn panel_name(&self) -> &'static str {
        "ResultGridPanel"
    }
}

impl Panel for ResultGridPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Result Grid".into_any_element()
    }
}
