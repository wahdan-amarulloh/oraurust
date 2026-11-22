//! Headless smoke test, run before any UI: can we load the vendored PL/SQL
//! grammar, parse a sample, and compile the zed-oracle-ext highlights query?
//! Prints styled-text stats; no window required.

use gpui_kit::component::highlighter::{LanguageConfig, LanguageRegistry};
use gpui_kit_spike::plsql;
use tree_sitter::{Parser, StreamingIterator as _};

const SAMPLE: &str = r#"
-- Employee salary report
CREATE OR REPLACE PACKAGE BODY emp_pkg AS

  g_min_salary CONSTANT NUMBER := 30000;
  g_dept_name  VARCHAR2(60) := 'Finance';

  PROCEDURE raise_salary(
    p_emp_id  IN employees.employee_id%TYPE,
    p_percent IN NUMBER DEFAULT 5
  ) IS
    v_current  NUMBER(10,2);
    v_message  VARCHAR2(200);
  BEGIN
    SELECT salary INTO v_current
      FROM employees
     WHERE employee_id = p_emp_id
       FOR UPDATE;

    IF v_current IS NULL THEN
      RAISE_APPLICATION_ERROR(-20001, 'No such employee: ' || p_emp_id);
    ELSIF p_percent > 50 THEN
      DBMS_OUTPUT.PUT_LINE('Refusing absurd raise');
      RETURN;
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

  FUNCTION dept_count(p_dept IN departments.department_id%TYPE) RETURN NUMBER IS
    v_count PLS_INTEGER := 0;
  BEGIN
    SELECT COUNT(*) INTO v_count FROM employees WHERE department_id = p_dept;
    RETURN v_count;
  END dept_count;

END emp_pkg;
/
"#;

fn main() {
    // 1. Load the grammar.
    let mut parser = Parser::new();
    parser
        .set_language(&plsql::language())
        .expect("grammar ABI mismatch with tree-sitter 0.26");

    // 2. Parse the sample.
    let tree = parser.parse(SAMPLE, None).expect("parse failed");
    let root = tree.root_node();
    println!(
        "parsed: {} bytes, root kind = {}, has_error = {}",
        SAMPLE.len(),
        root.kind(),
        root.has_error()
    );

    // 3. Compile the zed-oracle-ext highlights query against the grammar.
    let registry = LanguageRegistry::singleton();
    registry.register(
        plsql::LANG_NAME,
        &LanguageConfig::new(
            plsql::LANG_NAME,
            plsql::language(),
            vec![],
            plsql::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
    );
    let config = registry
        .language(plsql::LANG_NAME)
        .expect("config not registered");
    let query = tree_sitter::Query::new(&plsql::language(), &config.highlights)
        .expect("highlights query failed to compile");
    println!(
        "highlights query compiled: {} patterns, {} capture names",
        query.pattern_count(),
        query.capture_names().len()
    );

    // 4. Run the query and count highlighted ranges per capture.
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(&query, root, SAMPLE.as_bytes());
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    while let Some(m) = matches.next() {
        for cap in m.captures {
            *counts
                .entry(query.capture_names()[cap.index as usize].to_string())
                .or_default() += 1;
        }
    }
    println!("capture counts:");
    for (name, count) in &counts {
        println!("  {name}: {count}");
    }

    // 5. A quick sanity check: the `SELECT` keyword and the string literal
    //    must both have been captured.
    for expected in ["keyword", "string", "comment", "number", "type"] {
        assert!(
            counts.contains_key(expected),
            "expected capture {expected:?} missing — highlighting would be incomplete"
        );
    }
    println!("OK");
}
