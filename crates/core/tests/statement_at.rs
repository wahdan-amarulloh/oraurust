//! `statement_at`: the statement under the cursor (issue #6, Ctrl+Enter).

use oraurust_core::statement_at;

const TWO: &str = "select 1 from dual;\nselect 2 from dual;";

#[test]
fn cursor_inside_a_statement_picks_that_statement() {
    let statement = statement_at(TWO, 3).expect("cursor in the first statement");
    assert_eq!(statement.text, "select 1 from dual");

    let second_start = TWO.find("select 2").unwrap();
    let statement = statement_at(TWO, second_start + 4).expect("cursor in the second statement");
    assert_eq!(statement.text, "select 2 from dual");
}

#[test]
fn two_statements_on_one_line_disambiguate_by_nearest_text() {
    let script = "select 1 from dual; select 2 from dual;";

    // Right after the first `;` the first statement is nearer.
    let after_first = script.find(';').unwrap() + 1;
    assert_eq!(
        statement_at(script, after_first).unwrap().text,
        "select 1 from dual"
    );

    let statement = statement_at(script, script.len()).unwrap();
    assert_eq!(statement.text, "select 2 from dual");
}

#[test]
fn cursor_on_a_blank_line_picks_the_nearest_statement() {
    let script = "select 1 from dual;\n\n\nselect 2 from dual;\n\n\n";

    // Line 3 (blank) sits nearer the following statement.
    assert_eq!(statement_at(script, 21).unwrap().text, "select 2 from dual");
    // At EOF the last statement is nearest.
    assert_eq!(
        statement_at(script, script.len()).unwrap().text,
        "select 2 from dual"
    );
}

#[test]
fn cursor_on_a_plsql_blocks_terminator_picks_the_block() {
    let script = "begin\n  null;\nend;\n/\n";
    let slash = script.find('/').unwrap();
    assert_eq!(
        statement_at(script, slash).unwrap().text,
        "begin\n  null;\nend;"
    );
}

#[test]
fn empty_and_comment_only_scripts_have_no_statement_under_cursor() {
    assert!(statement_at("", 0).is_none());
    assert!(statement_at("-- nothing here\n", 5).is_none());
}

#[test]
fn cursor_offset_past_the_end_clamps_to_the_last_statement() {
    assert_eq!(
        statement_at("select 1 from dual;", 9999).unwrap().text,
        "select 1 from dual"
    );
}
