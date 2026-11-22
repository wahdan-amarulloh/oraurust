//! PL/SQL grammar + highlighting queries, wired into gpui-component's
//! `LanguageRegistry` and the tree-sitter highlight API.
//!
//! Grammar: `njank/tree-sitter-plsql` (MIT), the same grammar the
//! developer's `zed-oracle-ext` pins. Highlighting queries are the
//! extension's `highlights.scm`, adapted only in the header capture
//! (`@keyword` — gpui-component's registry has no `@header` name).

use tree_sitter::Language;

unsafe extern "C" {
    fn tree_sitter_plsql() -> Language;
}

/// The tree-sitter language compiled from the vendored C sources.
pub fn language() -> Language {
    unsafe { tree_sitter_plsql() }
}

/// The zed-oracle-ext highlights query, with Zed-only capture names mapped
/// onto the 41 names gpui-component's registry understands.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../grammar/highlights.scm");

/// Name under which the grammar is registered in gpui-component's registry.
pub const LANG_NAME: &str = "plsql";

/// Registers PL/SQL with gpui-component's highlighter so `EditorState`
/// `.language("plsql")` parses and highlights.
pub fn register() {
    gpui_kit::component::highlighter::LanguageRegistry::singleton().register(
        LANG_NAME,
        &gpui_kit::component::highlighter::LanguageConfig::new(
            LANG_NAME,
            language(),
            vec![],           // no injection languages
            HIGHLIGHTS_QUERY, // highlights
            "",               // injections
            "",               // locals
        ),
    );
}
