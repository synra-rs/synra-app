use std::sync::Arc;

use parking_lot::Mutex;
use synra_protocol::ids::PluginId;

use super::DesktopHost;

mod desktop;
mod network;

/// A plugin identifier, spelled the way the protocol spells it.
fn id(name: &str) -> PluginId {
    PluginId::parse(name).expect("a plugin id")
}

fn host() -> DesktopHost {
    let state = crate::testing::state();
    // ★ The plugin rows have to exist: `plugin_storage` references `plugins`, so a value stored against a plugin this
    // database has never heard of is refused by the schema. That is the cascade doing its job — and it is why the first
    // version of this test failed with `FOREIGN KEY constraint failed`, which is a better answer than silently keeping
    // storage for a plugin nobody installed.
    for plugin in ["@acme/notes", "@acme/chat"] {
        state
            .db
            .lock()
            .insert_plugin(plugin, true, true, "healthy", 1, 1)
            .expect("a plugin row");
    }
    DesktopHost::new(Arc::clone(&state.db))
}

pub(super) fn host_with(grants: &[(&str, bool)]) -> DesktopHost {
    let mut db = synra_store::SynraDb::open_in_memory().expect("a database");
    db.migrate().expect("migrates");
    db.insert_plugin("@acme/notes", true, true, "healthy", 1, 1)
        .expect("a plugin row");
    let decisions: Vec<(String, bool)> = grants
        .iter()
        .map(|(cap, granted)| ((*cap).to_owned(), *granted))
        .collect();
    db.record_grants("@acme/notes", &decisions, 1, "digest")
        .expect("decisions");
    DesktopHost::new(Arc::new(Mutex::new(db)))
}
