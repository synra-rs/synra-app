//! Opening a `.synra` package from the operating system.
//!
//! # What "file association" means here
//!
//! The installer registers this application for `.synra` files; the operating system then launches it with the file as
//! an argument. This module turns that argument list into the one thing the shell does next — install the package — and
//! deliberately ignores everything else: a flag, a directory, or a path that does not exist is not a package the user
//! double-clicked.

use std::cmp::Ordering;
use std::path::PathBuf;
use synra_plugin_api::manifest::{ManifestError, PluginVersion};

/// What opening a package should do, given what is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageAction {
    /// Nothing is installed at this identifier.
    Install,
    /// The package is newer than the active version.
    Update,
    /// The same version is already active; opening it again changes nothing.
    AlreadyInstalled,
    /// The active version is newer; a double-click is not an instruction to downgrade.
    NewerInstalled,
}

/// Decides what a package means against the version currently in effect.
///
/// # Errors
/// Reports malformed SemVer in the offered package or installed metadata.
pub fn action(installed: Option<&str>, offered: &str) -> Result<PackageAction, ManifestError> {
    let offered = PluginVersion::parse(offered)?;
    let Some(installed) = installed else {
        return Ok(PackageAction::Install);
    };
    let installed = PluginVersion::parse(installed)?;
    Ok(match installed.cmp_precedence(&offered) {
        Ordering::Less => PackageAction::Update,
        Ordering::Equal => PackageAction::AlreadyInstalled,
        Ordering::Greater => PackageAction::NewerInstalled,
    })
}

/// The `.synra` file an argument list names, if it names one.
///
/// The first argument is the executable and is skipped. The rest are scanned in order; the first existing file whose
/// extension is `synra` wins. Existence is checked because an association can be invoked with a stale path, and asking
/// the installer to open a missing file would report a source error for what is really a shell problem.
#[must_use]
pub fn package_from_args<I>(args: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let _program = args.next()?;
    args.map(PathBuf::from).find(|path| {
        path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("synra"))
    })
}

#[cfg(test)]
mod tests {
    use super::{PackageAction, action, package_from_args};

    #[test]
    fn a_package_is_installed_updated_or_left_alone_by_its_version() {
        assert_eq!(action(None, "1.0.0"), Ok(PackageAction::Install));
        assert_eq!(action(Some("1.0.0"), "2.0.0"), Ok(PackageAction::Update));
        assert_eq!(
            action(Some("1.9.0"), "1.10.0"),
            Ok(PackageAction::Update),
            "version order, not string order"
        );
        assert_eq!(
            action(Some("2.0.0"), "2.0.0"),
            Ok(PackageAction::AlreadyInstalled)
        );
        assert_eq!(
            action(Some("2.0.0"), "1.0.0"),
            Ok(PackageAction::NewerInstalled)
        );
        assert_eq!(
            action(Some("1.0.0-rc.2"), "1.0.0-rc.10"),
            Ok(PackageAction::Update)
        );
        assert_eq!(
            action(Some("1.0.0+build.1"), "1.0.0+build.9"),
            Ok(PackageAction::AlreadyInstalled)
        );
        assert!(action(Some("2026-11-27"), "1.0.0").is_err());
        assert!(action(None, "1.0.0-01").is_err());
    }

    #[test]
    fn the_package_argument_is_the_file_that_exists() {
        let root = std::env::temp_dir().join(format!(
            "synra-launch-{}-{}",
            std::process::id(),
            synra_store::sqlite::now_ms()
        ));
        std::fs::create_dir_all(&root).expect("a scratch directory");
        let package = root.join("hello.synra");
        std::fs::write(&package, b"not really a package").expect("a file");
        let missing = root.join("missing.synra");

        let args = vec![
            "synra-app.exe".to_owned(),
            "--some-flag".to_owned(),
            missing.display().to_string(),
            package.display().to_string(),
        ];
        assert_eq!(package_from_args(args), Some(package));

        // A directory with the right extension is not a package; the installer's source stage would refuse it, and this
        // is the earlier, clearer answer.
        let directory = root.join("folder.synra");
        std::fs::create_dir_all(&directory).expect("a directory");
        assert_eq!(
            package_from_args(["synra-app.exe".to_owned(), directory.display().to_string()]),
            None
        );
        assert_eq!(package_from_args(["synra-app.exe".to_owned()]), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}
