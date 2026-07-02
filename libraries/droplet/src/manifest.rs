use std::{collections::HashMap, ops::Not, path::Path};

use anyhow::anyhow;
use async_trait::async_trait;
pub use droplet_types::{ChunkData, FileEntry, Manifest};
use futures::StreamExt;
use hex::ToHex as _;
use humansize::{format_size, BINARY};
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncReadExt as _, AsyncWrite};
use tokio::sync::Semaphore;

pub const CHUNK_SIZE: u64 = 1024 * 1024 * 64;
pub const MAX_FILE_COUNT: usize = 512;

use crate::versions::{
    create_backend_constructor,
    types::{VersionBackend, VersionFile},
};

#[async_trait]
pub trait ManifestWriterFactory: Send + Sync {
    type Writer: AsyncWrite + Unpin;
    async fn create(&self, id: String) -> anyhow::Result<Self::Writer>;
    async fn close(&self, writer: Self::Writer) -> anyhow::Result<()>;
}

pub async fn generate_manifest_rusty<P, LogFn, ProgFn, Writer>(
    dir: P,
    progress_sfn: ProgFn,
    log_sfn: LogFn,
    factory: Option<&dyn ManifestWriterFactory<Writer = Writer>>,
    semaphore: Option<&Semaphore>,
) -> anyhow::Result<Manifest>
where
    P: AsRef<Path>,
    LogFn: Fn(String) + Clone,
    ProgFn: Fn(f32),
    Writer: AsyncWrite + Unpin,
{
    let backend = create_backend_constructor(dir).ok_or(anyhow!(
        "Could not create backend for path. Is this structure supported?"
    ))?()?;
    let mut files = backend.list_files().await?;
    files.sort_by_key(|b| std::cmp::Reverse(b.size));

    // Chunk boundaries don't align with file boundaries (a file can be split
    // across chunks, and a chunk can hold pieces of multiple files), and
    // chunks are hashed concurrently, so a per-file hash can't be derived
    // from the chunk hashing below. Hash whole files independently instead,
    // before `files` is consumed by `organise_files`.
    let files_for_hashing = files.clone();

    log_sfn("organising files into chunks...".to_string());

    let chunks = organise_files(files, backend.require_whole_files());

    log_sfn(format!(
        "organized into {} chunks, generating checksums...",
        chunks.len()
    ));
    let manifest = read_chunks_and_generate_manifest(
        backend.as_ref(),
        chunks,
        progress_sfn,
        &log_sfn,
        factory,
        semaphore,
    )
    .await?;

    log_sfn("hashing whole files for content verification...".to_string());
    let file_hashes = hash_whole_files(backend.as_ref(), files_for_hashing, semaphore).await?;

    let mut key = [0u8; 16];
    getrandom::fill(&mut key).map_err(|err| anyhow!("failed to generate key: {:?}", err))?;

    let total_manifest_length = manifest
        .values()
        .map(|value| value.files.iter().map(|f| f.length as u64).sum::<u64>())
        .sum::<u64>();

    Ok(Manifest {
        version: "2".to_string(),
        chunks: manifest,
        size: total_manifest_length,
        key,
        file_hashes,
    })
}

// Computes a whole-file SHA-256 (hex) per relative filename. Deliberately a
// second, independent read pass over each file (doubling per-file I/O at
// ingest time) rather than trying to reuse the per-chunk hashing above -
// that's a one-time admin-side cost, not something end users feel.
async fn hash_whole_files(
    backend: &(dyn VersionBackend + Send + Sync),
    files: Vec<VersionFile>,
    semaphore: Option<&Semaphore>,
) -> anyhow::Result<HashMap<String, String>> {
    let futures = files.into_iter().map(|file| async move {
        let permit = if let Some(semaphore) = &semaphore {
            Some(semaphore.acquire().await?)
        } else {
            None
        };

        let mut reader = backend.reader(&file, 0, file.size).await?;
        let mut hasher = Sha256::new();
        let mut read_buf = vec![0u8; 1024 * 1024];
        loop {
            let amount = reader.read(&mut read_buf).await?;
            if amount == 0 {
                break;
            }
            hasher.update(&read_buf[..amount]);
        }
        drop(permit);

        let hash: String = hasher.finalize().encode_hex();
        Ok::<_, anyhow::Error>((file.relative_filename, hash))
    });

    let mut stream = futures::stream::iter(futures)
        .buffer_unordered(semaphore.map(|s| s.available_permits()).unwrap_or(4));
    let mut results = HashMap::new();
    while let Some(res) = stream.next().await {
        let (filename, hash) = res?;
        results.insert(filename, hash);
    }
    Ok(results)
}

fn organise_files(
    files: Vec<VersionFile>,
    require_whole_files: bool,
) -> Vec<Vec<(VersionFile, u64, u64)>> {
    let mut chunks = Vec::new();
    let mut current_chunk = Vec::new();

    for version_file in files {
        if current_chunk.len() >= MAX_FILE_COUNT {
            // Pop current chunk
            chunks.push(std::mem::take(&mut current_chunk));
            println!("Chunks: {}", chunks.len());
        }
        let current_chunk_size = current_chunk
            .iter()
            .map(|(_, _, length)| *length)
            .sum::<u64>();
        let version_file_size = version_file.size;

        if require_whole_files {
            // If the current chunk is larger than chunk size, there's no point adding
            // it to the current_chunk. Just push it by itself
            if version_file_size >= CHUNK_SIZE {
                chunks.push(vec![(version_file, 0, version_file_size)]);
                println!("Chunks: {}", chunks.len());
                continue;
            }

            current_chunk.push((version_file, 0, version_file_size));
            if current_chunk_size + version_file_size >= CHUNK_SIZE {
                // Pop current chunk
                chunks.push(std::mem::take(&mut current_chunk));
                println!("Chunks: {}", chunks.len());
            }
        } else {
            // Enough space for it to be put in immediately
            if version_file_size + current_chunk_size < CHUNK_SIZE {
                current_chunk.push((version_file, 0, version_file_size));
                continue;
            }

            let bytes_free_in_existing_chunk = CHUNK_SIZE - current_chunk_size;
            current_chunk.push((version_file.clone(), 0, bytes_free_in_existing_chunk));
            chunks.push(std::mem::take(&mut current_chunk));

            // Loop over remaining data and create sufficient chunks to use it
            let mut offset = bytes_free_in_existing_chunk;
            while offset < version_file_size {
                let length = CHUNK_SIZE.min(version_file_size - offset);
                if length == CHUNK_SIZE {
                    chunks.push(vec![(version_file.clone(), offset, length)]);
                    println!("Chunks: {}", chunks.len());
                } else {
                    current_chunk.push((version_file.clone(), offset, length));
                    println!("Chunks: {}", chunks.len());
                }
                offset += length;
            }
        }
    }
    if current_chunk.is_empty().not() {
        chunks.push(current_chunk);
        println!("Pushed final chunk: {}", chunks.len());
    }
    println!("Chunks: {}", chunks.len());
    chunks
}

async fn read_chunks_and_generate_manifest<LogFn, ProgFn, Writer>(
    backend: &(dyn VersionBackend + Send + Sync),
    chunks: Vec<Vec<(VersionFile, u64, u64)>>,
    progress_sfn: ProgFn,
    log_sfn: &LogFn,
    factory: Option<&dyn ManifestWriterFactory<Writer = Writer>>,
    semaphore: Option<&Semaphore>,
) -> anyhow::Result<HashMap<String, ChunkData>>
where
    LogFn: Fn(String),
    ProgFn: Fn(f32),
    Writer: AsyncWrite + Unpin,
{
    let total_chunk_count = chunks.len();

    let futures = chunks.into_iter().enumerate().map(|(index, chunk)| {
        // To make the borrow checker happy
        async move {
            let mut read_buf = vec![0; 1024 * 1024 * 64];

            let uuid = uuid::Uuid::new_v4().to_string();
            let mut hasher = Sha256::new();

            let mut iv = [0u8; 16];
            getrandom::fill(&mut iv).map_err(|err| anyhow!("failed to generate IV: {:?}", err))?;
            let mut chunk_data = ChunkData {
                files: Vec::new(),
                checksum: String::new(),
                iv,
            };
            let mut chunk_length = 0u64;
            let mut writer = match factory {
                Some(factory) => Some(factory.create(uuid.clone()).await?),
                None => None,
            };
            for (file, start, length) in chunk {
                let permit = if let Some(semaphore) = &semaphore {
                    Some(semaphore.acquire().await?)
                } else {
                    None
                };
                chunk_data.files.push(
                    read_and_generate_chunk_file_data(
                        backend,
                        &file,
                        start,
                        length,
                        &mut hasher,
                        &mut read_buf,
                        &mut writer,
                    )
                    .await?,
                );
                chunk_length += length;
                drop(permit);
            }
            if let Some(factory) = factory {
                factory.close(writer.expect("Failed to get writer")).await?;
            }
            let hash: String = hasher.finalize().encode_hex();
            chunk_data.checksum = hash;

            log_sfn(format!(
                "created chunk of size {} ({}b) from {} files (index {})",
                format_size(chunk_length, BINARY),
                chunk_length,
                chunk_data.files.len(),
                index
            ));

            Ok::<_, anyhow::Error>((uuid, chunk_data))
        }
    });
    let mut stream = futures::stream::iter(futures)
        .buffer_unordered(semaphore.map(|s| s.available_permits()).unwrap_or(4))
        .enumerate();
    let mut results = HashMap::new();
    let mut current_progress = 0f32;
    while let Some((_, res)) = stream.next().await {
        let (id, data) = res?;
        current_progress += 1.0;
        progress_sfn((current_progress / total_chunk_count as f32) * 100.0f32);
        results.insert(id, data);
    }
    Ok(results)
}
async fn read_and_generate_chunk_file_data<Writer>(
    backend: &(dyn VersionBackend + Sync + Send),
    file: &VersionFile,
    start: u64,
    length: u64,
    hasher: &mut Sha256,
    read_buf: &mut [u8],
    writer: &mut Option<Writer>,
) -> anyhow::Result<FileEntry>
where
    Writer: AsyncWrite + Unpin,
{
    let mut reader = backend.reader(file, start, start + length).await?;

    loop {
        let amount = reader.read(read_buf).await?;

        if amount == 0 {
            break;
        }
        if let Some(writer) = writer.as_mut() {
            writer.write_all(&read_buf[0..amount]).await?;
        }
        hasher.update(&read_buf[0..amount]);
    }

    Ok(FileEntry {
        filename: file.relative_filename.clone(),
        start: start.try_into().unwrap(),
        length: length.try_into().unwrap(),
        permissions: file.permission,
    })
}
