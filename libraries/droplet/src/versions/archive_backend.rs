use std::{path::PathBuf, task::Poll};

use anyhow::anyhow;
use async_trait::async_trait;
use libarchive_drop::{
    archive::{Entry, FileType, ReadCompression, ReadFormat},
    reader::{Builder, FileReader, Reader},
};
use tokio::io::AsyncRead;

use crate::versions::types::{MinimumFileObject, VersionBackend, VersionFile};

pub struct ZipVersionBackend {
    path: PathBuf,
}
impl ZipVersionBackend {
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        Ok(Self { path })
    }

    fn open_archive(&self) -> Result<FileReader, anyhow::Error> {
        let mut archive = Builder::new();
        archive.support_format(ReadFormat::All)?;
        archive.support_compression(ReadCompression::All)?;
        let archive = archive.open_file(&self.path)?;

        Ok(archive)
    }
}

struct ArchiveReader {
    archive: FileReader,
    prev_block: Option<Vec<u8>>,
}

impl AsyncRead for ArchiveReader {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if let Some(block) = &mut self.prev_block {
            let to_read = buf.remaining().min(block.len());
            buf.put_slice(&block[..to_read]);
            block.drain(..to_read);

            // If the block is empty, we can read more
            if block.is_empty() {
                self.prev_block = None;
            } else {
                return Poll::Ready(Ok(()));
            }
        }
        let prev_block_update = {
            let block = match self.archive.read_block() {
                Ok(v) => v,
                Err(err) => return Poll::Ready(Err(std::io::Error::other(err.to_string()))),
            };

            let mut block = match block {
                Some(v) => v,
                None => return Poll::Ready(Ok(())),
            };

            let write_amount = buf.remaining().min(block.len());
            let to_write = block.split_off(..write_amount);
            let to_write = to_write.unwrap(); // SAFETY: above .min statement
            buf.put_slice(to_write);

            if !block.is_empty() {
                Some(block[buf.remaining()..].to_vec())
            } else {
                None
            }
        };

        if let Some(prev_block) = prev_block_update {
            self.prev_block.replace(prev_block);
        }

        Poll::Ready(Ok(()))
    }
}

#[async_trait]
impl VersionBackend for ZipVersionBackend {
    async fn list_files(&self) -> anyhow::Result<Vec<VersionFile>> {
        let mut archive = self.open_archive()?;
        let mut results = Vec::new();

        while let Some(header) = archive.next_header() {
            // A link inside an archive is carried as a link, the same as one
            // in a directory version: the archive holds its target, it has no
            // content of its own, and nothing it points at is ever read.
            let (size, symlink) = match header.filetype() {
                FileType::RegularFile => (header.size().try_into()?, None),
                FileType::SymbolicLink => {
                    let target = header.symlink().ok_or_else(|| {
                        anyhow!("no link target for {} in the archive", header.pathname())
                    })?;
                    (0, Some(target.to_owned()))
                }
                _ => continue,
            };

            results.push(VersionFile {
                relative_filename: header.pathname().to_string(),
                permission: 0o744,
                size,
                symlink,
            });
        }

        Ok(results)
    }

    async fn reader(
        &self,
        file: &VersionFile,
        _start: u64,
        _end: u64,
    ) -> anyhow::Result<Box<dyn MinimumFileObject>> {
        // A link has no bytes in the archive to find, and its target
        // travels in the manifest.
        if file.symlink.is_some() {
            return Ok(Box::new(tokio::io::empty()));
        }

        let mut archive = self.open_archive()?;

        // Find entry in archive
        loop {
            let entry = match archive.next_header() {
                Some(v) => v,
                None => return Err(anyhow!("entry not found:{}", file.relative_filename)),
            };
            if entry.pathname() == file.relative_filename {
                break;
            }
        }

        Ok(Box::new(ArchiveReader {
            archive,
            prev_block: None,
        }))
    }

    async fn peek_file(&self, sub_path: String) -> anyhow::Result<VersionFile> {
        let files = self.list_files().await?;
        let file = files
            .iter()
            .find(|v| v.relative_filename == sub_path)
            .ok_or_else(|| anyhow!("no {sub_path} in {}", self.path.display()))?;

        Ok(file.clone())
    }

    fn require_whole_files(&self) -> bool {
        true
    }
}
