//! Generate native ACL command groups from the same registry as the invoke handler.

use std::{fmt::Write as _, path::PathBuf};
use synra_ipc::{CommandRegistry, Interface};

pub(super) fn build() {
    if let Err(error) = generate() {
        eprintln!("could not generate Synra command permissions: {error}");
        std::process::exit(1);
    }
}

fn generate() -> Result<(), Box<dyn std::error::Error>> {
    CommandRegistry::verify()?;
    let directory = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo did not set OUT_DIR")?)
        .join("synra-permissions");
    std::fs::create_dir_all(&directory)?;
    let mut content = String::new();
    for (identifier, interface) in [
        ("host-commands", Interface::Host),
        ("plugin-ui-commands", Interface::Plugin),
    ] {
        writeln!(
            content,
            "[[permission]]\nidentifier = {identifier:?}\ncommands.allow = ["
        )?;
        for command in CommandRegistry::for_interface(interface) {
            writeln!(content, "  {:?},", command.name)?;
        }
        writeln!(content, "]\n")?;
    }
    let file = directory.join("commands.toml");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(content.as_str()) {
        std::fs::write(file, content)?;
    }
    // Tauri's build API requires a static glob; this allocation lives only for this build-script process.
    let pattern = format!("{}/*.toml", directory.to_string_lossy().replace('\\', "/"));
    let pattern = Box::leak(pattern.into_boxed_str());
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().permissions_path_pattern(pattern)),
    )?;
    Ok(())
}
