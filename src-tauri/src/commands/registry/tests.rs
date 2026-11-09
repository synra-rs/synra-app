use super::publisher_mismatch;
use std::collections::BTreeMap;

#[test]
fn a_relative_tarball_is_resolved_against_the_index() {
    assert_eq!(
        super::resolve_tarball(
            "https://bucket.example/plugins/@acme/hello.json",
            "hello.tgz"
        ),
        Ok("https://bucket.example/plugins/@acme/hello.tgz".to_owned())
    );
    assert_eq!(
        super::resolve_tarball(
            "https://bucket.example/plugins/@acme/hello.json",
            "/x/hello.tgz"
        ),
        Ok("https://bucket.example/x/hello.tgz".to_owned())
    );
    assert_eq!(
        super::resolve_tarball("https://cdn.example/x.json", "https://other.example/y.tgz"),
        Ok("https://other.example/y.tgz".to_owned())
    );
}

#[test]
fn a_relative_tarball_may_not_climb_out_of_its_directory() {
    // ★ `..` is refused rather than normalised: an index that walks out of its own directory is asking for a fetch the
    // author did not mean to publish.
    assert!(
        super::resolve_tarball("https://bucket.example/a/hello.json", "../secret.tgz").is_err()
    );
    assert!(super::resolve_tarball("https://bucket.example/a/hello.json", "").is_err());
}

#[test]
fn a_directory_key_that_matches_is_silent() {
    let mut publishers = BTreeMap::new();
    publishers.insert("@acme".to_owned(), "AB12".to_owned());
    assert_eq!(
        publisher_mismatch(&publishers, Some("@acme"), Some("AB12")),
        None
    );
}

#[test]
fn a_directory_key_that_differs_is_a_sentence() {
    // ★ The sentence names both values: a person can only judge the disagreement if it shows what disagreed.
    let mut publishers = BTreeMap::new();
    publishers.insert("@acme".to_owned(), "AB12".to_owned());

    let warning = publisher_mismatch(&publishers, Some("@acme"), Some("CD34")).expect("a warning");

    assert!(warning.contains("AB12"), "{warning}");
    assert!(warning.contains("CD34"), "{warning}");
    assert!(warning.contains("@acme"), "{warning}");
}

#[test]
fn nothing_to_compare_is_not_a_mismatch() {
    let mut publishers = BTreeMap::new();
    publishers.insert("@acme".to_owned(), "AB12".to_owned());
    assert_eq!(
        publisher_mismatch(&publishers, Some("@other"), Some("CD34")),
        None
    );
    assert_eq!(publisher_mismatch(&publishers, Some("@acme"), None), None);
    assert_eq!(publisher_mismatch(&publishers, None, Some("AB12")), None);
}

fn static_document() -> String {
    serde_json::json!({
        "name": "@acme/hello",
        "latest": "1.0.0",
        "publishers": { "@acme": "AB12" },
        "versions": [{ "version": "1.0.0", "tarball": "hello.tgz" }]
    })
    .to_string()
}

fn headers() -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("etag".to_owned(), "\"v1\"".to_owned());
    headers
}

/// Sets a file's modification time, so a test can make it look old.
fn set_modified(path: &std::path::Path, at_ms: i64) {
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("the file");
    let time = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(u64::try_from(at_ms).expect("a positive time"));
    file.set_times(std::fs::FileTimes::new().set_modified(time))
        .expect("an mtime");
}

#[test]
fn a_document_survives_the_process_that_fetched_it() {
    // ★ The point of the disk copy is the next process: validators and body together, so it can revalidate instead of
    // downloading the same document again.
    let (_root, state) = crate::testing::file_state();
    let url = "https://bucket.example/plugins/@acme/hello.json";
    super::store_document(
        &state,
        url,
        super::Kind::Static,
        &static_document(),
        &headers(),
        None,
    );

    let loaded = super::load_document(&state, url, super::Kind::Static, 0).expect("loaded");

    assert_eq!(loaded.etag.as_deref(), Some("\"v1\""));
    assert_eq!(loaded.document.latest().expect("a latest").version, "1.0.0");
    assert_eq!(
        loaded.publishers.get("@acme").map(String::as_str),
        Some("AB12")
    );
}

#[test]
fn an_expired_document_is_removed_rather_than_served() {
    let (_root, state) = crate::testing::file_state();
    let url = "https://bucket.example/plugins/@acme/hello.json";
    super::store_document(
        &state,
        url,
        super::Kind::Static,
        &static_document(),
        &headers(),
        Some(10),
    );

    assert!(super::load_document(&state, url, super::Kind::Static, 11).is_none());
    assert!(
        !super::document_cache_path(&state, url).exists(),
        "the stale file is gone"
    );
}

#[test]
fn a_document_nobody_read_for_a_month_is_removed() {
    // ★ A document with only an `ETag` has no expiry, so the load path never deletes it. Without this prune the
    // directory keeps one file per registry this device ever asked about.
    let (_root, state) = crate::testing::file_state();
    let url = "https://bucket.example/plugins/@acme/hello.json";
    super::store_document(
        &state,
        url,
        super::Kind::Static,
        &static_document(),
        &headers(),
        None,
    );
    let path = super::document_cache_path(&state, url);
    let file = std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("the file");
    file.set_times(std::fs::FileTimes::new().set_modified(
        std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 24 * 3600),
    ))
    .expect("an old mtime");
    drop(file);

    let removed = super::prune_documents(&state, synra_store::sqlite::now_ms());

    assert!(removed >= 1);
    assert!(!path.exists());
}

#[test]
fn a_fresh_document_is_kept() {
    let (_root, state) = crate::testing::file_state();
    let url = "https://bucket.example/plugins/@acme/hello.json";
    super::store_document(
        &state,
        url,
        super::Kind::Static,
        &static_document(),
        &headers(),
        None,
    );
    let path = super::document_cache_path(&state, url);

    let removed = super::prune_documents(&state, synra_store::sqlite::now_ms());

    assert_eq!(removed, 0);
    assert!(path.exists());
}

#[test]
fn the_oldest_documents_are_dropped_when_the_cache_is_too_large() {
    // ★ Time alone leaves a script that asks about thousands of registries in one month with a directory of thousands of
    // documents. The oldest go first, and only until the ceiling is met.
    let directory =
        std::env::temp_dir().join(format!("synra-registry-prune-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a directory");
    let now = synra_store::sqlite::now_ms();
    let oldest = directory.join("oldest.json");
    let newest = directory.join("newest.json");
    std::fs::write(&oldest, b"0123456789").expect("an old file");
    std::fs::write(&newest, b"0123456789").expect("a new file");
    set_modified(&oldest, now - 1_000_000);
    set_modified(&newest, now);

    let removed = super::prune_directory(&directory, now, 10);

    assert_eq!(removed, 1);
    assert!(!oldest.exists(), "the oldest document is the one that went");
    assert!(newest.exists());
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_document_of_another_format_is_ignored() {
    // ★ A future format is not this format. Ignoring it (and refetching) is right; parsing it as if it were the same
    // shape is how a cache turns into a bug that looks like a registry problem.
    let (_root, state) = crate::testing::file_state();
    let url = "https://bucket.example/plugins/@acme/hello.json";
    super::store_document(
        &state,
        url,
        super::Kind::Static,
        &static_document(),
        &headers(),
        None,
    );
    let path = super::document_cache_path(&state, url);
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("the file")).expect("JSON");
    value["version"] = serde_json::json!(999);
    std::fs::write(&path, value.to_string()).expect("the changed file");

    assert!(super::load_document(&state, url, super::Kind::Static, 0).is_none());
}
