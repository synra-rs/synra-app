use super::{
    AUDIT_RETENTION_KEY, announce_updated, discover_plugins, install_directory,
    load_installed_plugins, plugins_audit, plugins_audit_clear, plugins_catalog_search,
    plugins_check_update, plugins_consent_respond, plugins_install, plugins_list, plugins_preview,
    plugins_publisher_decide, plugins_publishers, prune_audit, record_card,
};
use crate::testing;
use synra_ipc::{CatalogQuery, InstallSourceDto};
use synra_protocol::PluginId;

const ID: &str = "@synra-plugin/chat";

/// A state with a scripted engine, because installing needs one and the fixture builds a shell without.
///
/// The backend answers from a table rather than compiling Wasm: what is being tested is the shell's command — the
/// source it accepts, the report it returns and the row that lands — and the pipeline behind it has its own tests.
fn with_engine(state: crate::state::AppState) -> crate::state::AppState {
    with_scripted_engine(state).0
}

/// The same engine, with the backend kept so a test can script one entry point.
fn with_scripted_engine(
    state: crate::state::AppState,
) -> (crate::state::AppState, synra_plugin_host::ScriptedBackend) {
    let host = std::sync::Arc::new(crate::plugin_host::DesktopHost::with_platform(
        std::sync::Arc::clone(&state.db),
        std::sync::Arc::clone(&state.platform),
    ));
    let scripted = synra_plugin_host::ScriptedBackend::new();
    let backend = synra_plugin_host::Backend::new(std::sync::Arc::new(scripted.clone()));
    (
        state.with_plugins(std::sync::Arc::new(parking_lot::Mutex::new(
            synra_core::dispatch::PluginSet::new(backend, host),
        ))),
        scripted,
    )
}

/// A package directory a local install will accept: a manifest and a component.
fn package(root: &std::path::Path) -> std::path::PathBuf {
    let directory = root.join("package");
    std::fs::create_dir_all(&directory).expect("a package directory");
    let manifest = format!(
        r#"{{"id": "@acme/hello", "version": "1.0.0", "title": "Hello",
                "publisher": {{"id": "@acme"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
        synra_plugin_host::host_world()
    );
    std::fs::write(directory.join("manifest.json"), manifest).expect("a manifest");
    std::fs::write(
        directory.join("plugin.wasm"),
        synra_testkit::component::inert_component().expect("a valid component"),
    )
    .expect("an artifact");
    directory
}

/// A `.tgz` holding these entries, written in memory.
fn tgz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *bytes)
            .expect("an entry");
    }
    builder
        .into_inner()
        .expect("the encoder")
        .finish()
        .expect("a gzip tail")
}

/// A tiny HTTP server for the registry tests: answers each request from `handler`, and counts what it served.
fn serve(
    handler: impl Fn(&str) -> (u16, Vec<u8>) + Send + 'static,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{BufRead, BufReader, Write as _};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let address = listener.local_addr().expect("an address");
    let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&served);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let (status, body) = handler(&first);
            let reason = if status == 200 { "OK" } else { "Not Found" };
            let head = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (format!("http://{address}"), served)
}

/// A manifest declaring the two capabilities the consent tests decide about.
///
/// The shape is the one `synra-plugin-api`'s own fixture uses, because a hand-written manifest that drifted from it
/// would test the parser rather than this command.
const MANIFEST: &str = r#"{
        "id": "@synra-plugin/chat",
        "version": "1.0.0",
        "title": "Chat",
        "publisher": { "id": "synra-official" },
        "sdk": { "min": "1.0.0" },
        "wit": "synra:plugin@1.0.0",
        "runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false } },
        "capabilities": [{ "cap": "log:*" }, { "cap": "device:list" }]
    }"#;

/// A state with the plugin installed on disk and in the database, awaiting consent.
fn installed() -> (testing::Root, crate::state::AppState) {
    let (root, state) = testing::file_state();
    let directory = state.paths.plugin_dir(ID);
    std::fs::create_dir_all(&directory).expect("the plugin directory");
    std::fs::write(directory.join("manifest.json"), MANIFEST).expect("a manifest");
    state
        .db()
        .insert_plugin(
            ID,
            true,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    (root, state)
}

fn grants(state: &crate::state::AppState) -> Vec<(String, bool)> {
    state
        .db()
        .plugin_grants(ID)
        .expect("the decisions")
        .into_iter()
        .map(|row| (row.capability, row.granted))
        .collect()
}

/// A recorded row, filled in with the fields the mapping reads.
fn row(
    plugin: &str,
    capability: &str,
    outcome: &str,
    at_ms: i64,
) -> synra_store::records::AuditRecord {
    synra_store::records::AuditRecord {
        plugin_id: plugin.to_owned(),
        capability: capability.to_owned(),
        outcome: outcome.to_owned(),
        at_ms,
        elapsed_ms: Some(7),
        trace_id: None,
    }
}

/// Puts a loadable plugin on disk and records the decision that lets it run.
///
/// ★ The directory comes from `Paths::plugin_dir`, the same function the installer, removal and sweep use. A fixture
/// that joined the raw id here would put the manifest somewhere the loader never looks, and the test would then be
/// asserting a layout no installation has.
fn write_loadable_files(state: &crate::state::AppState) -> String {
    let manifest = format!(
        r#"{{"id": "{ID}", "version": "1.0.0", "title": "Chat",
                "publisher": {{"id": "synra-official"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
        synra_plugin_host::host_world()
    );
    let directory = state.paths.plugin_dir(ID);
    std::fs::create_dir_all(&directory).expect("the plugin directory");
    std::fs::write(directory.join("manifest.json"), &manifest).expect("a manifest");
    std::fs::write(
        directory.join("plugin.wasm"),
        synra_testkit::component::inert_component().expect("a valid component"),
    )
    .expect("an artifact");
    manifest
}

fn loadable(state: &crate::state::AppState) {
    let manifest = write_loadable_files(state);
    state
        .db()
        .insert_plugin(
            ID,
            true,
            true,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    state
        .db()
        .record_grants(
            ID,
            &[("log:*".to_owned(), true)],
            testing::FIXED_NOW_MS,
            &synra_plugin_api::manifest_digest(&manifest),
        )
        .expect("the decisions");
}

/// A directory package for a newer version, as the registry updater unpacks one.
fn write_update_source(root: &std::path::Path, version: &str) -> std::path::PathBuf {
    let directory = root.join(format!("update-{version}"));
    std::fs::create_dir_all(&directory).expect("a package directory");
    let manifest = format!(
        r#"{{"id": "{ID}", "version": "{version}", "title": "Chat",
                "publisher": {{"id": "synra-official"}}, "sdk": {{"min": "0.1.0"}},
                "wit": "{}",
                "capabilities": [{{"cap": "log:*"}}],
                "runtime": {{"headless": {{"artifact": "plugin.wasm", "stateful": false}}}}}}"#,
        synra_plugin_host::host_world()
    );
    std::fs::write(directory.join("manifest.json"), manifest).expect("a manifest");
    std::fs::write(
        directory.join("plugin.wasm"),
        synra_testkit::component::inert_component().expect("a valid component"),
    )
    .expect("an artifact");
    directory
}

mod activation;
mod audit;
mod authority;
mod cache_npm;
mod cache_static;
mod cards;
mod catalogue;
mod install_local;
mod install_registry;
mod uninstall;
mod updates;
