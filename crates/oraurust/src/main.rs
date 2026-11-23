//! oraurust: a PL/SQL desktop editor for Oracle 11g (spec: issue #1).
//!
//! Issue #3 — app shell: a GPUI window with a dock layout (editor pane in
//! the center, left sidebar for the future Schema Browser, bottom panel for
//! the Result Grid). Issue #6 — Ctrl+Enter executes the statement under the
//! cursor on the background executor and its output lands in the Result Grid.

mod connection_panel;
mod oracle_conn;
mod plsql;
mod result_grid;
mod runner;
mod session;
mod workspace;

use gpui_kit::assets::Assets;
use gpui_kit::{
    AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, open_window, px, size,
};

fn main() {
    // Register PL/SQL with the highlighter before any editor renders.
    plsql::register();

    let app = gpui_kit::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        cx.activate(true);

        // Compile the highlights query at startup; a broken query otherwise
        // fails silently as a grey editor (lesson from the issue #2 spike).
        {
            let registry = gpui_kit::component::highlighter::LanguageRegistry::singleton();
            let config = registry
                .language(plsql::LANG_NAME)
                .expect("plsql config registered");
            let _ = tree_sitter::Query::new(&plsql::language(), &config.highlights)
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

        let _ = open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    window_size,
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("oraurust".into()),
                    appears_transparent: false,
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| workspace::Workspace::new(window, cx)),
        )
        .expect("failed to open oraurust window");
    });
}
