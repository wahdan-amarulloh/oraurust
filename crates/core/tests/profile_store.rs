use oraurust_core::{Profile, ProfileStore};

fn temp_store() -> (tempfile::TempDir, ProfileStore) {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = ProfileStore::new(dir.path().join("profiles.json"));
    (dir, store)
}

#[test]
fn save_then_load_roundtrips_profiles_across_restarts() {
    let (_dir, store) = temp_store();
    let mut profile = Profile::new("dev", "db.example.com", 1521, "orcl", "scott");
    profile.password = "tiger".into();
    store.save(&profile).expect("save");

    // A fresh store over the same path stands in for an app restart.
    let reopened = ProfileStore::new(store.path().to_path_buf());
    let loaded = reopened.load().expect("load");
    assert_eq!(loaded, vec![profile]);
}

#[test]
fn password_never_appears_plaintext_in_the_store_file() {
    let (_dir, store) = temp_store();
    let mut profile = Profile::new("dev", "db.example.com", 1521, "orcl", "scott");
    profile.password = "hunter2-secret".into();
    store.save(&profile).expect("save");

    let raw = std::fs::read(store.path()).expect("read store file");
    let text = String::from_utf8_lossy(&raw);
    assert!(
        !text.contains("hunter2-secret"),
        "password must be encrypted at rest, file was:\n{text}"
    );
}

#[test]
fn save_with_existing_name_edits_in_place() {
    let (_dir, store) = temp_store();
    let mut profile = Profile::new("dev", "db.example.com", 1521, "orcl", "scott");
    profile.password = "one".into();
    store.save(&profile).expect("save");

    let mut edited = profile.clone();
    edited.host = "db2.example.com".into();
    edited.password = "two".into();
    store.save(&edited).expect("edit");

    let loaded = store.load().expect("load");
    assert_eq!(loaded, vec![edited], "editing must not duplicate the profile");
}

#[test]
fn delete_removes_only_the_named_profile() {
    let (_dir, store) = temp_store();
    let mut a = Profile::new("a", "h1", 1521, "s1", "u1");
    a.password = "p".into();
    let mut b = Profile::new("b", "h2", 1521, "s2", "u2");
    b.password = "p".into();
    store.save(&a).expect("save a");
    store.save(&b).expect("save b");

    store.delete("a").expect("delete");
    assert_eq!(store.load().expect("load"), vec![b]);
}

#[test]
fn load_of_missing_store_is_empty() {
    let (_dir, store) = temp_store();
    assert!(store.load().expect("load").is_empty());
}
