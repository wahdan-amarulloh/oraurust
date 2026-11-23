//! CSV export of a Result Set (issue #7: "Export any grid to CSV with correct
//! headers and values").
//!
//! Format is RFC 4180: comma-separated fields, CRLF record endings, a field
//! quoted when it contains a comma, a double quote or a line break, and an
//! embedded double quote doubled. `NULL` is an empty, unquoted field; an
//! empty string is quoted (`""`), so the two stay distinguishable.

use oraurust_core::{Column, ResultSet, to_csv};

fn columns(names: &[(&str, &str)]) -> Vec<Column> {
    names
        .iter()
        .map(|(name, type_name)| Column {
            name: (*name).into(),
            type_name: (*type_name).into(),
        })
        .collect()
}

#[test]
fn header_then_one_line_per_row() {
    let result = ResultSet {
        columns: columns(&[("ID", "NUMBER"), ("NAME", "VARCHAR2")]),
        rows: vec![
            vec![Some("1".into()), Some("Alice".into())],
            vec![Some("2".into()), Some("Bob".into())],
        ],
    };

    assert_eq!(to_csv(&result), "ID,NAME\r\n1,Alice\r\n2,Bob\r\n");
}

#[test]
fn null_is_an_empty_field_and_an_empty_string_is_quoted() {
    let result = ResultSet {
        columns: columns(&[("A", "VARCHAR2"), ("B", "VARCHAR2"), ("C", "VARCHAR2")]),
        rows: vec![vec![None, Some(String::new()), Some("x".into())]],
    };

    assert_eq!(to_csv(&result), "A,B,C\r\n,\"\",x\r\n");
}

#[test]
fn fields_with_commas_quotes_or_line_breaks_are_quoted_and_escaped() {
    let result = ResultSet {
        columns: columns(&[("TEXT", "VARCHAR2")]),
        rows: vec![
            vec![Some("a,b".into())],
            vec![Some("say \"hi\"".into())],
            vec![Some("line1\nline2".into())],
            vec![Some("carriage\rreturn".into())],
        ],
    };

    assert_eq!(
        to_csv(&result),
        "TEXT\r\n\"a,b\"\r\n\"say \"\"hi\"\"\"\r\n\"line1\nline2\"\r\n\"carriage\rreturn\"\r\n"
    );
}

#[test]
fn a_result_with_no_rows_still_carries_the_header() {
    let result = ResultSet {
        columns: columns(&[("ID", "NUMBER")]),
        rows: Vec::new(),
    };

    assert_eq!(to_csv(&result), "ID\r\n");
}

#[test]
fn a_row_shorter_than_the_header_is_padded_with_nulls() {
    // Every record must have as many fields as the header, or a CSV reader
    // misaligns the rest of the file. A short row is padded with empty fields.
    let result = ResultSet {
        columns: columns(&[("A", "NUMBER"), ("B", "NUMBER"), ("C", "NUMBER")]),
        rows: vec![vec![Some("1".into())]],
    };

    assert_eq!(to_csv(&result), "A,B,C\r\n1,,\r\n");
}

#[test]
fn an_empty_result_exports_nothing() {
    assert_eq!(to_csv(&ResultSet::default()), "");
}
