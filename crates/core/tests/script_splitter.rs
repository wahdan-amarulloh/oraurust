use oraurust_core::split;

#[test]
fn splits_a_semicolon_terminated_statement() {
    let statements = split("select 1 from dual;");

    assert_eq!(statements.len(), 1);
    assert_eq!(statements[0].text, "select 1 from dual");
}

#[test]
fn non_ascii_punctuation_does_not_panic() {
    // A multi-byte character where a word would start used to slice mid-char
    // while sniffing for a PL/SQL block keyword.
    let statements = split("select 1 from t — ok;\nselect 2 from dual;");

    assert_eq!(statements.len(), 2);
    assert_eq!(statements[0].text, "select 1 from t — ok");
    assert_eq!(statements[1].text, "select 2 from dual");
}

#[test]
fn splits_mixed_semicolon_statements_and_slash_blocks() {
    // Spec user story 7: a whole script with mixed SQL and PL/SQL runs as-is.
    let script = "\
delete from tmp_log;

create or replace procedure p is
begin
  null;
end;
/

select sysdate from dual;
";
    let statements = split(script);

    assert_eq!(statements.len(), 3);
    assert_eq!(statements[0].text, "delete from tmp_log");
    assert_eq!(
        statements[1].text,
        "create or replace procedure p is\nbegin\n  null;\nend;"
    );
    assert_eq!(statements[1].start_line, 3);
    assert_eq!(statements[1].end_line, 6);
    assert_eq!(statements[2].text, "select sysdate from dual");
}

#[test]
fn semicolons_inside_string_literals_and_comments_do_not_split() {
    let script = "\
insert into notes values ('a;b', 'it''s; here'); -- harmless; here
/* block; comment */
select 1 from dual;
";
    let statements = split(script);

    assert_eq!(statements.len(), 2);
    assert_eq!(
        statements[0].text,
        "insert into notes values ('a;b', 'it''s; here')"
    );
    assert_eq!(statements[1].text, "select 1 from dual");
}

#[test]
fn comment_only_and_empty_scripts_yield_no_statements() {
    assert!(split("").is_empty());
    assert!(split("   \n\t\n  ").is_empty());
    assert!(split("-- just a comment\n").is_empty());
    assert!(split("/* nothing\nhere */").is_empty());
}

#[test]
fn line_anchored_slash_requires_nothing_else_on_the_line() {
    // A `/` with other text on the line is division, not a terminator.
    let script = "select 10 / 2 from dual;\nselect 2 from dual";
    let statements = split(script);

    assert_eq!(statements.len(), 2);
    assert_eq!(statements[0].text, "select 10 / 2 from dual");
    assert_eq!(statements[1].text, "select 2 from dual");
}

#[test]
fn plsql_block_at_eof_without_slash_is_one_statement() {
    // Script ends inside a PL/SQL block with no `/` terminator: the block is
    // still one statement (its internal `;` do not split, EOF flushes it).
    let statements = split("begin\n  null;\nend;");

    assert_eq!(statements.len(), 1);
    assert!(statements[0].text.ends_with("end;"));
    assert_eq!(statements[0].start_line, 1);
    assert_eq!(statements[0].end_line, 3);
}

#[test]
fn trailing_whitespace_after_last_statement_is_dropped() {
    let statements = split("select 1 from dual;\n\n   \n");
    assert_eq!(statements.len(), 1);
    assert_eq!(statements[0].text, "select 1 from dual");
}
