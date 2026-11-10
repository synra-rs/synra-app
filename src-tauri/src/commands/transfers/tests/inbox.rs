#[test]
fn the_received_list_reads_the_inbox_and_skips_installed_bundles() {
    // ★ The list is the directory, not a table: a row that outlived the file would send a user to a path that is gone.
    let (_root, state) = crate::testing::file_state();
    let inbox = state.paths.data_dir().join(synra_core::inbound::INBOX_DIR);
    std::fs::create_dir_all(&inbox).expect("the inbox");
    std::fs::write(inbox.join("notes.txt"), b"hello").expect("a file");
    std::fs::write(inbox.join("photo.png"), b"png").expect("a file");
    std::fs::write(inbox.join("plugin-@acme+chat.synra"), b"bundle").expect("a bundle");

    let files = super::super::transfer_received(&state);

    let names: Vec<&str> = files.iter().map(|file| file.file_name.as_str()).collect();
    assert!(names.contains(&"notes.txt"), "{names:?}");
    assert!(names.contains(&"photo.png"), "{names:?}");
    assert!(
        !names.iter().any(|name| name.ends_with(".synra")),
        "an installed bundle is not a received file"
    );
    let notes = files
        .iter()
        .find(|file| file.file_name == "notes.txt")
        .expect("notes");
    assert_eq!(notes.bytes, 5);
    assert!(notes.path.ends_with("notes.txt"), "{}", notes.path);
}

#[test]
fn opening_a_received_file_is_contained_to_the_inbox() {
    // ★ `open_path` launches whatever the OS associates with the path. Without this check a webview could name any file on
    // the disk and have the user open it; the inbox is the only place a received file is allowed to be.
    let (_root, state) = crate::testing::file_state();
    let inbox = state.paths.data_dir().join(synra_core::inbound::INBOX_DIR);
    std::fs::create_dir_all(&inbox).expect("the inbox");
    let inside = inbox.join("notes.txt");
    std::fs::write(&inside, b"hello").expect("a received file");
    let outside = state.paths.data_dir().join("identity.toml");
    std::fs::write(&outside, b"secret").expect("a file outside the inbox");

    assert!(super::super::inbox_file(&state, &inside.to_string_lossy()).is_ok());
    assert_eq!(
        super::super::inbox_file(&state, &outside.to_string_lossy())
            .expect_err("outside is refused")
            .code,
        synra_protocol::ErrorCode::InvalidArgument
    );
    assert_eq!(
        super::super::inbox_file(&state, &inbox.join("missing.txt").to_string_lossy())
            .expect_err("missing is refused")
            .code,
        synra_protocol::ErrorCode::InvalidArgument
    );
}

#[test]
fn discarding_a_received_file_removes_only_an_inbox_file() {
    // ★ Deleting by path is the one command here that can destroy something. The containment check is what keeps it from
    // being "delete any file the webview can name"; the outside file is asserted to still exist.
    let (_root, state) = crate::testing::file_state();
    let inbox = state.paths.data_dir().join(synra_core::inbound::INBOX_DIR);
    std::fs::create_dir_all(&inbox).expect("the inbox");
    let inside = inbox.join("done.txt");
    std::fs::write(&inside, b"goodbye").expect("a received file");
    let outside = state.paths.data_dir().join("identity.toml");
    std::fs::write(&outside, b"keep me").expect("a file outside the inbox");

    assert_eq!(
        super::super::transfer_discard(&state, &outside.to_string_lossy())
            .expect_err("outside is refused")
            .code,
        synra_protocol::ErrorCode::InvalidArgument
    );
    assert!(outside.exists(), "the refusal deleted the wrong file");

    super::super::transfer_discard(&state, &inside.to_string_lossy()).expect("the inbox file is removed");
    assert!(!inside.exists(), "the received file is still there");
}
