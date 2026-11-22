//! PL/SQL grammar + highlighting queries, wired into gpui-kit's
//! `LanguageRegistry`.
//!
//! Grammar: `njank/tree-sitter-plsql` (MIT), the same grammar the
//! developer's `zed-oracle-ext` pins. `grammar/highlights.scm` is a copy of
//! the extension's highlights query; it uses only capture names that
//! gpui-kit's registry understands, so no remapping is needed.
//!
//! The grammar's own Rust binding pins tree-sitter ~0.25.9 and its build
//! script has the external scanner commented out, so `parser.c` /
//! `scanner.c` are vendored under `grammar/plsql/` and compiled by
//! `build.rs` against tree-sitter 0.26.13 (validated by the issue #2 spike).

use tree_sitter::Language;

unsafe extern "C" {
    fn tree_sitter_plsql() -> Language;
}

/// The tree-sitter language compiled from the vendored C sources.
pub fn language() -> Language {
    unsafe { tree_sitter_plsql() }
}

/// The zed-oracle-ext highlights query. Uses gpui-kit capture names directly.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../grammar/highlights.scm");

/// Name under which the grammar is registered in gpui-kit's registry.
pub const LANG_NAME: &str = "plsql";

/// Registers PL/SQL with gpui-kit's highlighter so `EditorState`
/// `.language("plsql")` parses and highlights.
pub fn register() {
    gpui_kit::component::highlighter::LanguageRegistry::singleton().register(
        LANG_NAME,
        &gpui_kit::component::highlighter::LanguageConfig::new(
            LANG_NAME,
            language(),
            vec![],
            HIGHLIGHTS_QUERY,
            "",
            "",
        ),
    );
}
