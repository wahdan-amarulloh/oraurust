//! Compiles the vendored `njank/tree-sitter-plsql` grammar (MIT).
//!
//! The grammar's own Rust binding pins `tree-sitter ~0.25.9` and its build
//! script has the external scanner commented out, so it cannot be used as a
//! dependency. We vendor `src/parser.c`, `src/scanner.c` and the C headers
//! verbatim and compile them here against the runtime the editor uses
//! (tree-sitter 0.26.13, which accepts the ABI 15 parser).

fn main() {
    let grammar = std::path::Path::new("grammar/plsql");

    let mut build = cc::Build::new();
    build
        .include(grammar)
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");

    for source in ["parser.c", "scanner.c"] {
        let path = grammar.join(source);
        build.file(&path);
        println!("cargo:rerun-if-changed={}", path.display());
    }

    build.compile("tree-sitter-plsql");
}
