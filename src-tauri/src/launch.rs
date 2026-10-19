//! Opening a `.synra` package from the operating system.
//!
//! # What "file association" means here
//!
//! The installer registers this application for `.synra` files; the operating system then launches it with the file as
//! an argument. This module turns that argument list into the one thing the shell does next — install the package — and
//! deliberately ignores everything else: a flag, a directory, or a path that does not exist is not a package the user
//! double-clicked.

use std::path::PathBuf;

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
    args.map(PathBuf::from)
        .find(|path| path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("synra")))
}

#[cfg(test)]
mod tests {
    use super::package_from_args;

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
        assert_eq!(package_from_args(["synra-app.exe".to_owned(), directory.display().to_string()]), None);
        assert_eq!(package_from_args(["synra-app.exe".to_owned()]), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}
