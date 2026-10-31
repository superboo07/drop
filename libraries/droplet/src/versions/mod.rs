use std::{
    fs::{metadata, read_dir},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};

use crate::versions::{path_backend::PathVersionBackend, types::VersionBackend};

use crate::versions::archive_backend::ZipVersionBackend;

// libarchive backend is Linux-only for now
pub mod archive_backend;
pub mod path_backend;

/// Collects every entry under `path`, descending into directories.
///
/// Nothing inside the directory is followed: `file_type` comes from the
/// directory entry and so describes a symbolic link itself, which means a
/// link is collected as an entry of its own and is never descended into or
/// read through. Resolving them instead made a link pointing at something
/// that isn't there -- ordinary enough inside a game -- fail the whole
/// listing with "No such file or directory", leaving the version impossible
/// to import, and meant a link was stored as a second copy of its target.
///
/// `path` itself is resolved, since that is the directory we were asked to
/// list -- a library root or a version is often reached through a link.
///
/// Errors name the path they happened on, so a directory the OS won't let
/// us read says which one it was.
pub fn _list_files(vec: &mut Vec<PathBuf>, path: &Path) -> Result<()> {
    let at = |p: &Path| format!("at {}", p.display());

    if metadata(path).with_context(|| at(path))?.is_dir() {
        let paths = read_dir(path).with_context(|| at(path))?;
        for path_result in paths {
            let entry = path_result.with_context(|| at(path))?;
            let full_path = entry.path();
            // Only ever true for a real directory, so the recursive call
            // above resolving `path` can't follow a link either.
            if entry.file_type().with_context(|| at(&full_path))?.is_dir() {
                _list_files(vec, &full_path)?;
            } else {
                vec.push(full_path);
            }
        }
    };

    Ok(())
}

const SUPPORTED_FILE_EXTENSIONS: [&str; 11] = [
    "tar", "pax", "cpio", "zip", "jar", "ar", "xar", "rar", "rpm", "7z", "iso",
];

pub mod types;

type BackendConstructor<'a> =
    Box<dyn FnOnce() -> Result<Box<dyn VersionBackend + Send + Sync + 'a>>>;

pub fn create_backend_constructor<'a, P>(path: P) -> Option<BackendConstructor<'a>>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    if !path.exists() {
        return None;
    }

    let is_directory = path.is_dir();
    if is_directory {
        let base_dir = path.to_path_buf();
        return Some(Box::new(move || {
            Ok(Box::new(PathVersionBackend { base_dir }))
        }));
    };

    let file_extension = path.extension().and_then(|v| v.to_str())?;
    if SUPPORTED_FILE_EXTENSIONS.contains(&file_extension) {
        let buf = path.to_path_buf();
        return Some(Box::new(move || Ok(Box::new(ZipVersionBackend::new(buf)?))));
    }

    None
}
