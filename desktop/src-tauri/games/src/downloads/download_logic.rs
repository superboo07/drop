use std::collections::{HashMap, HashSet};
use std::io::{self, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::{Duration, Instant};

use aes::cipher::{KeyIvInit, StreamCipher};
use download_manager::error::ApplicationDownloadError;
use download_manager::util::download_thread_control_flag::{
    DownloadThreadControl, DownloadThreadControlFlag,
};
use download_manager::util::progress_object::ProgressHandle;
use droplet_types::ChunkData;
use futures_util::StreamExt as _;
use log::{debug, info, warn};
use remote::auth::generate_authorization_header;
use remote::error::{DropServerError, RemoteAccessError};
use remote::utils::DROP_CLIENT_ASYNC;
use sha2::Digest;
use tauri::Url;
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _};
use tokio_util::io::StreamReader;

const READ_BUF_LEN: usize = 1024 * 1024;

// Chunk bodies can legitimately take far longer than the client's default
// request timeout to fully stream (reqwest's timeout covers body download,
// not just headers) - especially large chunks on slower connections. Rather
// than bound the whole transfer, bound how long we'll wait for *any*
// progress: a read stall this long means the connection is actually dead.
const CHUNK_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(25);
// Outer safety net against a pathologically slow-but-technically-alive
// connection, so a single chunk can't hang a download forever.
const CHUNK_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);

type Aes128Ctr64LE = ctr::Ctr64LE<aes::Aes128>;

#[allow(clippy::too_many_arguments)]
pub async fn download_game_chunk(
    game_id: &str,
    version_id: &str,
    chunk_id: &str,
    depot: &str,
    key: &[u8; 16],
    chunk_data: &ChunkData,
    file_list: &HashMap<String, String>,
    // Only these files are written; the rest of the chunk is read past.
    // Keeps a chunk shared with a file the player chose to keep from
    // overwriting it.
    write_set: &HashSet<String>,
    base_path: &Path,
    control_flag: &DownloadThreadControl,
    // How much we're downloading
    download_progress: &ProgressHandle,
    // How much we're writing to disk
    disk_progress: &ProgressHandle,
) -> Result<bool, ApplicationDownloadError> {
    // If we're paused
    if control_flag.get() == DownloadThreadControlFlag::Stop {
        download_progress.set(0);
        disk_progress.set(0);
        return Ok(false);
    }

    let start = Instant::now();

    let header = generate_authorization_header();

    let url = Url::parse(depot)
        .map_err(|v| ApplicationDownloadError::DownloadError(v.into()))?
        .join(&format!("content/{}/{}/{}", game_id, version_id, chunk_id))
        .map_err(|v| ApplicationDownloadError::DownloadError(v.into()))?;

    let response = DROP_CLIENT_ASYNC
        .get(url)
        .header("Authorization", header)
        .timeout(CHUNK_DOWNLOAD_TIMEOUT)
        .send()
        .await
        .map_err(|e| ApplicationDownloadError::Communication(e.into()))?;

    if response.status() != 200 {
        info!("chunk request got status code: {}", response.status());
        // The depot doesn't have this chunk at all. That's not a transient
        // communication failure - the manifest we're downloading from is out
        // of date with the depot's content, and every retry of every chunk it
        // lists will 404 the same way. Report it as its own error so the
        // download agent can resync instead of failing the download forever.
        if response.status() == 404 {
            warn!(
                "depot {depot} has no chunk {chunk_id} for {game_id}/{version_id}: manifest is out of sync with depot content"
            );
            return Err(ApplicationDownloadError::ContentOutOfSync);
        }
        let raw_res = response.text().await.map_err(|e| {
            ApplicationDownloadError::Communication(RemoteAccessError::FetchErrorLegacy(e.into()))
        })?;
        info!("{raw_res}");
        if let Ok(err) = serde_json::from_str::<DropServerError>(&raw_res) {
            return Err(ApplicationDownloadError::Communication(
                RemoteAccessError::InvalidResponse(err),
            ));
        }
        return Err(ApplicationDownloadError::Communication(
            RemoteAccessError::UnparseableResponse(raw_res),
        ));
    }

    if control_flag.get() == DownloadThreadControlFlag::Stop {
        download_progress.set(0);
        disk_progress.set(0);
        return Ok(false);
    }

    let timestep = start.elapsed().as_millis();

    debug!("took {}ms to start downloading", timestep);

    let stream = response
        .bytes_stream()
        .map(|v| v.map_err(std::io::Error::other));
    let mut stream_reader = StreamReader::new(stream);
    //let mut stream_reader = response;

    let mut hasher = sha2::Sha256::new();
    let mut cipher = Aes128Ctr64LE::new(key.into(), &chunk_data.iv.into());
    let mut read_buf = vec![0u8; READ_BUF_LEN];
    for file in &chunk_data.files {
        let should_write = write_set.contains(&file.filename)
            && file_list
                .get(&file.filename)
                .map(|v| v == version_id)
                .unwrap_or(false);
        let path = base_path.join(file.filename.clone());
        if should_write && let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| at(parent, "couldn't create the directory", e))?;
        }

        // A link is restored as a link. It has no bytes in the chunk at all
        // (the manifest carries its target instead), so there is nothing to
        // read past either.
        if let Some(target) = &file.symlink {
            if should_write {
                restore_symlink(&path, target)
                    .await
                    .map_err(|e| at(&path, "couldn't create the link at", e))?;
            }
            continue;
        }

        let mut file_handle = if should_write {
            let mut file_handle = open_for_write(&path)
                .await
                .map_err(|e| at(&path, "couldn't open", e))?;
            file_handle
                .seek(SeekFrom::Start(file.start.try_into().unwrap()))
                .await
                .map_err(|e| at(&path, "couldn't seek in", e))?;
            Some(file_handle)
        } else {
            None
        };

        let mut remaining = file.length;
        while remaining > 0 {
            let amount = tokio::time::timeout(
                CHUNK_READ_IDLE_TIMEOUT,
                stream_reader.read(&mut read_buf[0..remaining.min(READ_BUF_LEN)]),
            )
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "stalled while downloading chunk")
            })??;
            // A zero-length read is EOF, not a stall: the response body was
            // shorter than the manifest said this file would be. Without this
            // the loop spins forever at 100% CPU waiting for bytes that are
            // never coming.
            if amount == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "chunk {chunk_id} ended {remaining} bytes short of the expected length for {}",
                        file.filename
                    ),
                )
                .into());
            }
            download_progress.add(amount);
            remaining -= amount;

            cipher.apply_keystream(&mut read_buf[0..amount]);
            hasher.update(&read_buf[0..amount]);
            if let Some(file_handle) = &mut file_handle {
                file_handle
                    .write_all(&read_buf[0..amount])
                    .await
                    .map_err(|e| at(&path, "couldn't write to", e))?;
                disk_progress.add(amount);
            }
        }

        if control_flag.get() == DownloadThreadControlFlag::Stop {
            download_progress.set(0);
            return Ok(false);
        }
    }

    let digest = hex::encode(hasher.finalize());
    if digest != chunk_data.checksum {
        return Err(ApplicationDownloadError::Checksum);
    }

    Ok(true)
}

/// Names the file an io error happened to, since the error itself only ever
/// says what went wrong ("permission denied") and a download touches
/// thousands of paths.
fn at(path: &Path, doing: &str, err: io::Error) -> io::Error {
    io::Error::new(err.kind(), format!("{doing} {}: {err}", path.display()))
}

/// Opens a file for writing, clearing a read-only mode left behind by an
/// earlier install of the same game if that's what's in the way.
///
/// Game content legitimately carries modes without an owner-write bit, and
/// the download applies those once everything is written -- but the file we
/// have to write into may already be wearing one from the install we're
/// replacing, or from a download that got as far as applying them before it
/// was interrupted.
async fn open_for_write(path: &Path) -> Result<tokio::fs::File, io::Error> {
    let mut options = tokio::fs::OpenOptions::new();
    options
        .truncate(false)
        .write(true)
        .append(false)
        .create(true);

    match options.open(path).await {
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            #[cfg(unix)]
            if let Ok(metadata) = tokio::fs::metadata(path).await {
                let mode = metadata.permissions().mode() | 0o200;
                tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await?;
                return options.open(path).await;
            }
            Err(err)
        }
        other => other,
    }
}

/// Recreates a symbolic link exactly as the server reported it, target
/// verbatim -- resolved or not, inside the game or out of it. Game files are
/// provided as they are.
#[cfg(unix)]
async fn restore_symlink(path: &Path, target: &str) -> Result<(), io::Error> {
    // `symlink` refuses to replace anything, and the link may already be
    // there from an earlier attempt or an earlier install.
    match tokio::fs::remove_file(path).await {
        Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
        _ => {}
    }

    tokio::fs::symlink(target, path).await
}

/// Windows needs a privilege to create a link and distinguishes file from
/// directory links, so the link is left as the empty file it has always been
/// written as there.
#[cfg(not(unix))]
async fn restore_symlink(path: &Path, _target: &str) -> Result<(), io::Error> {
    tokio::fs::OpenOptions::new()
        .truncate(false)
        .write(true)
        .create(true)
        .open(path)
        .await
        .map(|_| ())
}
