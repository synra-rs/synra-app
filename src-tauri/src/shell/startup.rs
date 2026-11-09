use synra_store::{Paths, SynraDb};
use tauri::Manager;

use crate::state::AppState;

use super::{
    clock::now_ms, discovery::with_discovery_or_report, plugins::with_plugins_or_refuse,
    wire::with_wire_trace,
};

pub(super) fn app_state(app: &tauri::AppHandle) -> Result<AppState, String> {
    let startup = std::time::Instant::now();
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("the data directory could not be found: {error}"))?;
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("the cache directory could not be found: {error}"))?;

    let paths = Paths::new(data_dir, cache_dir);
    paths
        .ensure_directories()
        .map_err(|error| format!("the data directories could not be created: {error}"))?;

    // ★ The crash pipeline starts before anything that can panic.
    //
    // A report of what went wrong has to be written by the layer that is already working, so the hook goes in before the
    // database is opened and before any plugin is compiled. The daemon installs the same hook at the same point; without
    // it here, a plugin that kills the window leaves no report, the verdict below can never see a crash loop, and safe
    // mode — the only way back to the settings screen a crashing plugin has made unreachable — never triggers.
    let crashes = synra_observe::crash::CrashStore::new(paths.log_dir());
    synra_observe::crash::install_panic_hook(crashes.clone(), env!("CARGO_PKG_VERSION"));
    // The verdict reads what previous runs left behind. It is taken once, here, and passed to the loader: a second read
    // later would see this run's own startup and answer a different question.
    let verdict = crashes.verdict(now_ms());

    let mut db = SynraDb::open(&paths.db_path())
        .map_err(|error| format!("the database could not be opened: {error}"))?;
    // Migrated on the way in, so a shell that starts on an older database does not fail every command with a message
    // about a missing table.
    db.migrate()
        .map_err(|error| format!("the database could not be migrated: {error}"))?;

    // ★ The platform string the kernel stores, taken from the build target rather than from `std::env::consts`.
    //
    // They usually agree, and they disagree in the cross-compiled case — and the identity records the platform at first
    // launch, so a wrong answer here is written to disk and then believed by every peer for the life of the install.
    //
    // ★ Android is the case that made this matter.
    //
    // `std::env::consts::OS` answered `"android"` for this build all along; what it did not answer was a string the
    // kernel's own vocabulary has a word for. `identity` stores what a peer is told, and a peer that is told "unknown"
    // records a device it can never name again — so the arm exists and says "android".
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };

    let state = AppState::new(db, paths, platform, now_ms())
        .map_err(|error| format!("the identity could not be prepared: {error:?}"))?;

    let plugins_started = std::time::Instant::now();
    let mut state = with_plugins_or_refuse(state, &verdict);
    // ★ The plugin pass is the part M4's cold-start budget is most likely to blow: every enabled plugin is compiled and
    // activated before the window appears. Measured separately from the state so a number that grows points at a cause.
    state.metrics.histogram_observe(
        &synra_observe::metrics::SeriesKey::new(
            "startup_duration_ms",
            [("phase".to_owned(), "plugins".to_owned())],
        ),
        plugins_started.elapsed().as_secs_f64() * 1_000.0,
    );
    // ★ Bind the listener **before** discovery, so the announcement can carry the port a peer should dial. The order is the
    // whole fix: announcing the discovery port meant every pairing dialled the responder, which accepts datagrams and not links.
    state = crate::listener::bind(state);
    // ★ And the half the shell never had: its own discovery. Started before the log so a bind failure is reported through
    // the log if one is running, and its failure is not fatal — see the function.
    state = with_discovery_or_report(state);
    // ★ The shell's own log, started before anything else can go wrong — and its failure is reported rather than fatal.
    // A window that refuses to open because it cannot write a diagnostic file trades the user's actual work for its
    // record, and `diag_export` says the section is absent instead of claiming there was nothing to log.
    if let Err(error) = state.start_logging() {
        eprintln!("synra-app: logging is not running: {error}");
    }

    // ★ The whole path from the first line of `app_state` to here, which is what "cold start" means for this shell: the
    // window has not been shown yet, and everything the first paint depends on has run.
    state.metrics.histogram_observe(
        &synra_observe::metrics::SeriesKey::new(
            "startup_duration_ms",
            [("phase".to_owned(), "total".to_owned())],
        ),
        startup.elapsed().as_secs_f64() * 1_000.0,
    );

    state = with_wire_trace(state);
    Ok(state)
}
