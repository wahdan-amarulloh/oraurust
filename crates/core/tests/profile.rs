use oraurust_core::Profile;

#[test]
fn builds_easy_connect_string_from_host_port_service() {
    let profile = Profile::new("dev", "db.example.com", 1521, "orcl", "scott");
    assert_eq!(
        profile.connect_string(),
        "//db.example.com:1521/orcl",
        "host/port/service must form the Easy Connect string the oracle crate accepts"
    );
}

#[test]
fn service_field_holds_service_name_or_sid_as_given() {
    // The profile does not distinguish service vs SID (strict spec, issue #5);
    // whatever the user typed goes into the Easy Connect path position.
    let sid_profile = Profile::new("old", "10.0.0.5", 1521, "ORCL", "system");
    assert_eq!(sid_profile.connect_string(), "//10.0.0.5:1521/ORCL");
}
