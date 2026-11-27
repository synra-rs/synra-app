use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[cfg(any(feature = "tauri-shell", test))]
use uuid::Uuid;

use super::budget::Lease;

const SLOT_PREFIX: &str = "upload-";

pub(crate) fn root(cache: &Path) -> PathBuf {
    cache.join("plugin-transfers")
}

#[derive(Debug, Clone)]
pub(super) struct Paths {
    directory: PathBuf,
    file: PathBuf,
}

impl Paths {
    pub(super) fn cleanup(&self) -> io::Result<()> {
        ignore_missing(fs::remove_file(&self.file))?;
        ignore_missing(fs::remove_dir(&self.directory))
    }
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Exclusive ownership of an admitted upload's disk contents and reservation.
#[derive(Debug)]
pub(crate) struct UploadSpool {
    paths: Paths,
    lease: Lease,
}

impl UploadSpool {
    pub(super) fn create(root: &Path, name: &str, lease: Lease) -> io::Result<(Self, fs::File)> {
        fs::create_dir_all(root)?;
        let directory = root.join(format!("{SLOT_PREFIX}{}", lease.id));
        fs::create_dir(&directory)?;
        let paths = Paths {
            file: directory.join(name),
            directory,
        };
        let spool = Self { paths, lease };
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(spool.path())?;
        Ok((spool, file))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.paths.file
    }
}

impl Drop for UploadSpool {
    fn drop(&mut self) {
        if let Err(error) = self.paths.cleanup() {
            eprintln!(
                "synra: upload {} could not be reclaimed: {error}",
                self.lease.id
            );
            self.lease.defer_cleanup(self.paths.clone());
        }
    }
}

/// Run after acquiring the application's single-instance lock. Only our one-file slots qualify.
#[cfg(any(feature = "tauri-shell", test))]
pub(crate) fn sweep_startup(cache: &Path) {
    let directory = root(cache);
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!("synra: could not inspect the upload spool: {error}");
            return;
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_prefix(SLOT_PREFIX))
            .and_then(|name| name.parse::<Uuid>().ok())
        else {
            continue;
        };
        if id.get_version_num() != 4 || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        if let Err(error) = sweep_slot(&entry.path()) {
            eprintln!("synra: could not reclaim upload {id} at start-up: {error}");
        }
    }
}

#[cfg(any(feature = "tauri-shell", test))]
fn sweep_slot(directory: &Path) -> io::Result<()> {
    let entries = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
    match entries.as_slice() {
        [] => fs::remove_dir(directory),
        [entry] if entry.file_type()?.is_file() => Paths {
            directory: directory.to_owned(),
            file: entry.path(),
        }
        .cleanup(),
        _ => Ok(()),
    }
}
