#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{io::SeekFrom, path::PathBuf};

use anyhow::{anyhow, Context as _};
use async_trait::async_trait;
use tokio::{
    fs::File,
    io::{AsyncReadExt as _, AsyncSeekExt as _},
};

#[derive(Clone)]
pub struct PathVersionBackend {
    pub base_dir: PathBuf,
}

use crate::versions::{
    _list_files,
    types::{MinimumFileObject, VersionBackend, VersionFile},
};

#[async_trait]
impl VersionBackend for PathVersionBackend {
    async fn list_files(&self) -> anyhow::Result<Vec<VersionFile>> {
        let mut vec = Vec::new();
        _list_files(&mut vec, &self.base_dir)?;

        let mut results = Vec::new();

        for pathbuf in vec.iter() {
            let relative = pathbuf.strip_prefix(self.base_dir.clone())?;

            results.push(
                self.peek_file(
                    relative
                        .to_str()
                        .ok_or(anyhow!(
                            "Could not parse path: {}",
                            relative.to_string_lossy()
                        ))?
                        .to_owned(),
                )
                .await
                .with_context(|| format!("at {}", pathbuf.display()))?,
            );
        }

        Ok(results)
    }

    async fn reader(
        &self,
        file: &VersionFile,
        start: u64,
        end: u64,
    ) -> anyhow::Result<Box<dyn MinimumFileObject>> {
        let pathbuf = self.base_dir.join(file.relative_filename.clone());

        // A link's content is its target, which is carried in the manifest
        // rather than as bytes. Opening it here would read whatever it points
        // at and serve that as the link's content -- or fail outright when
        // the target isn't there, as a virtualenv's `bin/python3` does.
        //
        // The flag alone isn't enough to go on: torrential builds a
        // `VersionFile` for chunk serving out of a manifest that reached it
        // over a protocol with no field for a link, so whether this is one is
        // settled on disk as well. A link is never followed either way.
        if file.symlink.is_some() || is_symlink(&pathbuf).await {
            return Ok(Box::new(tokio::io::empty()));
        }

        let mut file = File::open(&pathbuf)
            .await
            .with_context(|| format!("couldn't open {}", pathbuf.display()))?;

        if start != 0 {
            file.seek(SeekFrom::Start(start)).await?;
        }

        if end != 0 {
            return Ok(Box::new(file.take(end - start)));
        }

        Ok(Box::new(file))
    }

    async fn peek_file(&self, sub_path: String) -> anyhow::Result<VersionFile> {
        let pathbuf = self.base_dir.join(sub_path.clone());

        // Describes a link itself rather than its target, so a link that
        // points at something missing is still a file we can report.
        let metadata = tokio::fs::symlink_metadata(&pathbuf)
            .await
            .with_context(|| format!("couldn't read {}", pathbuf.display()))?;

        let permission_object = metadata.permissions();
        let permissions = {
            let perm: u32;
            #[cfg(target_family = "unix")]
            {
                perm = permission_object.mode();
            }
            #[cfg(not(target_family = "unix"))]
            {
                perm = 0
            }
            perm
        };

        if metadata.is_symlink() {
            let target = tokio::fs::read_link(&pathbuf)
                .await
                .with_context(|| format!("couldn't read the link at {}", pathbuf.display()))?;

            return Ok(VersionFile {
                relative_filename: sub_path,
                permission: permissions,
                // A link carries its target, not bytes of its own.
                size: 0,
                symlink: Some(
                    target
                        .to_str()
                        .ok_or(anyhow!(
                            "Could not parse link target: {}",
                            target.to_string_lossy()
                        ))?
                        .to_owned(),
                ),
            });
        }

        Ok(VersionFile {
            relative_filename: sub_path,
            permission: permissions,
            size: metadata.len(),
            symlink: None,
        })
    }

    fn require_whole_files(&self) -> bool {
        false
    }
}

/// Whether `path` is itself a symbolic link. A path that can't be stated at
/// all isn't reported as one, so opening it raises the real error.
async fn is_symlink(path: &std::path::Path) -> bool {
    tokio::fs::symlink_metadata(path)
        .await
        .map(|metadata| metadata.is_symlink())
        .unwrap_or(false)
}
