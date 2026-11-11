use super::*;

pub(super) struct RegisteredApp {
    pub(super) id: String,
    name: String,
    path: PathBuf,
}

pub(super) fn registered_apps() -> Vec<RegisteredApp> {
    #[cfg(target_os = "windows")]
    {
        let roots = [
            std::env::var_os("ProgramData")
                .map(PathBuf::from)
                .map(|root| root.join("Microsoft\\Windows\\Start Menu\\Programs")),
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .map(|root| root.join("Microsoft\\Windows\\Start Menu\\Programs")),
        ];
        let mut paths = Vec::new();
        for root in roots.into_iter().flatten() {
            collect_registered_app_paths(&root, 0, &mut paths);
        }
        paths.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
        paths.dedup();
        return paths
            .into_iter()
            .filter_map(|path| {
                let name = path.file_stem()?.to_string_lossy().trim().to_owned();
                if name.is_empty() {
                    return None;
                }
                Some(RegisteredApp {
                    id: registered_app_id(&path),
                    name,
                    path,
                })
            })
            .collect();
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "windows")]
pub(super) fn collect_registered_app_paths(root: &Path, depth: usize, paths: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_registered_app_paths(&path, depth + 1, paths);
            continue;
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if matches!(
            extension.to_ascii_lowercase().as_str(),
            "lnk" | "url" | "exe"
        ) {
            paths.push(path);
        }
    }
}

pub(super) fn registered_app_id(path: &Path) -> String {
    let mut hash = 2_166_136_261_u32;
    for byte in path.to_string_lossy().to_ascii_lowercase().bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    format!("synra-app-{hash:08x}")
}

pub(super) fn app_capability(app: &RegisteredApp) -> String {
    format!(
        "synra-app:{}\t{}",
        app.id,
        app.name.replace(['\t', '\r', '\n'], " ")
    )
}

#[cfg(target_os = "windows")]
pub(super) fn open_registered_app(id: &str) -> Result<(), CallError> {
    let Some(app) = registered_apps().into_iter().find(|app| app.id == id) else {
        return Err(CallError::new(
            CallError::BAD_ARGUMENT,
            "the application id is not registered on this computer",
        ));
    };
    std::process::Command::new("explorer.exe")
        .arg(app.path)
        .spawn()
        .map_err(|error| {
            CallError::new(
                CallError::HOST_FAILED,
                format!("could not open application: {error}"),
            )
        })?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(super) fn open_registered_app(_id: &str) -> Result<(), CallError> {
    Err(not_implemented("registered applications"))
}
