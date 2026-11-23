//! Readable Oracle errors (issue #6, "ORA- codes surface readably").

use oraurust_core::DbError;

#[test]
fn db_error_displays_code_message_and_position() {
    let error = DbError {
        code: "ORA-01756".into(),
        message: "quoted string not properly terminated".into(),
        position: Some(19),
    };
    assert_eq!(
        error.to_string(),
        "ORA-01756: quoted string not properly terminated (at position 19)"
    );
}

#[test]
fn db_error_without_code_or_position_shows_the_message_alone() {
    let error = DbError {
        code: String::new(),
        message: "connection lost".into(),
        position: None,
    };
    assert_eq!(error.to_string(), "connection lost");
}

#[test]
fn new_strips_a_code_the_message_already_carries() {
    // ODPI-C's message is "ORA-04043: object X does not exist"; without the
    // strip, Display printed the code twice.
    let error = DbError::new(
        "ORA-04043",
        "ORA-04043: object DUMMY does not exist",
        Some(2),
    );
    assert_eq!(error.message, "object DUMMY does not exist");
    assert_eq!(
        error.to_string(),
        "ORA-04043: object DUMMY does not exist (at position 2)"
    );
}

#[test]
fn new_strips_rust_oracles_oci_error_prefix() {
    let error = DbError::new(
        "ORA-04043",
        "OCI Error: ORA-04043: object DUMMY does not exist",
        None,
    );
    assert_eq!(error.to_string(), "ORA-04043: object DUMMY does not exist");
}

#[test]
fn new_keeps_a_message_that_does_not_repeat_the_code() {
    let error = DbError::new(
        "DPI-1047",
        "Cannot locate a 64-bit Oracle Client library",
        None,
    );
    assert_eq!(
        error.to_string(),
        "DPI-1047: Cannot locate a 64-bit Oracle Client library"
    );
}
