use synra_ipc::ConfigPatch;

mod announcements;
mod snapshots;
mod validation;

fn set(kv: &[(&str, serde_json::Value)]) -> ConfigPatch {
    ConfigPatch {
        set: kv
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
        unset: Vec::new(),
    }
}

fn unset(keys: &[&str]) -> ConfigPatch {
    ConfigPatch {
        set: Default::default(),
        unset: keys.iter().map(|k| (*k).to_owned()).collect(),
    }
}
