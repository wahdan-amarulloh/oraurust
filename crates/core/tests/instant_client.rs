use oraurust_core::instant_client::{ClientStatus, check, classify_error, remediation};
use oraurust_core::DbError;

#[test]
fn missing_client_when_no_search_path_holds_oci_dll() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(check(&[dir.path().to_path_buf()]), ClientStatus::Missing);
}

#[test]
fn finds_oci_dll_in_a_search_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("oci.dll"), b"stub").expect("write");
    assert_eq!(
        check(&[dir.path().to_path_buf()]),
        ClientStatus::Found(dir.path().to_path_buf())
    );
}

#[test]
fn remediation_names_the_package_and_a_download_link() {
    let text = remediation();
    assert!(text.contains("Instant Client"), "{text}");
    assert!(text.contains("Basic"), "{text}");
    assert!(text.contains("oracle.com"), "{text}");
}

#[test]
fn missing_client_error_becomes_actionable() {
    // ODPI-C's wording when no client library is found (DPI-1047).
    let raw = DbError {
        code: "DPI-1047".into(),
        message: "Cannot locate a 64-bit Oracle Client library".into(),
        position: None,
    };
    let err = classify_error(raw).expect("recognised as a client problem");
    assert!(err.message.contains("Instant Client"), "{}", err.message);
    assert!(err.message.contains("oracle.com"), "{}", err.message);
}

#[test]
fn incompatible_client_version_is_reported_as_such() {
    let raw = DbError {
        code: "DPI-1072".into(),
        message: "Cannot load the OCI library: the library is not compatible".into(),
        position: None,
    };
    let err = classify_error(raw).expect("recognised as a client problem");
    assert!(err.message.to_lowercase().contains("version"), "{}", err.message);
}

#[test]
fn ordinary_oracle_errors_pass_through_untouched() {
    assert!(classify_error(DbError {
        code: "ORA-12154".into(),
        message: "TNS:could not resolve the connect identifier specified".into(),
        position: None,
    })
    .is_none());
}
