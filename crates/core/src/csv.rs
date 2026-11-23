//! CSV export of a Result Set (issue #7: "Export any grid to CSV with correct
//! headers and values").
//!
//! RFC 4180: comma-separated fields, CRLF record endings, a field quoted when
//! it contains a comma, a double quote or a line break, and an embedded double
//! quote doubled.
//!
//! CSV has no NULL. `NULL` is written as a bare empty field, and an empty
//! string is quoted (`""`), so the two stay distinguishable on re-import.

use crate::connection::ResultSet;

/// The Result Set as a CSV document: a header row of column names, then one
/// record per row. Every record has exactly as many fields as the header (a
/// short row is padded with empty fields), so no reader misaligns. An empty
/// Result Set exports nothing.
pub fn to_csv(result: &ResultSet) -> String {
    if result.columns.is_empty() {
        return String::new();
    }
    let mut csv = String::new();
    for (ix, column) in result.columns.iter().enumerate() {
        if ix > 0 {
            csv.push(',');
        }
        push_field(&mut csv, Some(&column.name));
    }
    csv.push_str("\r\n");
    for row in &result.rows {
        for ix in 0..result.columns.len() {
            if ix > 0 {
                csv.push(',');
            }
            push_field(&mut csv, row.get(ix).and_then(Option::as_deref));
        }
        csv.push_str("\r\n");
    }
    csv
}

/// Appends one field. `None` (SQL NULL) is a bare empty field; `Some("")` is
/// quoted so a NULL and an empty string never collide.
fn push_field(csv: &mut String, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    if value.is_empty() || value.contains([',', '"', '\n', '\r']) {
        csv.push('"');
        for ch in value.chars() {
            if ch == '"' {
                csv.push('"');
            }
            csv.push(ch);
        }
        csv.push('"');
    } else {
        csv.push_str(value);
    }
}
