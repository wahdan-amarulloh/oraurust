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
