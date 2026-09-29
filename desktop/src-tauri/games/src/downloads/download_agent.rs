use async_trait::async_trait;
use database::models::data::UserConfiguration;
use database::{
    ApplicationTransientStatus, DownloadableMetadata, borrow_db_checked, borrow_db_mut_checked,
};
use download_manager::depot_manager::DepotManager;
use download_manager::download_manager_frontend::{DownloadManagerSignal, DownloadStatus};
use download_manager::downloadable::Downloadable;
use download_manager::error::ApplicationDownloadError;
use download_manager::util::download_thread_control_flag::{
    DownloadThreadControl, DownloadThreadControlFlag,
};
use download_manager::util::progress_object::{ProgressHandle, ProgressObject, ProgressType};
use droplet_types::{ChunkData, Manifest};
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use log::{debug, error, info, warn};
use remote::auth::generate_authorization_header;
use remote::cache::get_cached_object;
use remote::error::RemoteAccessError;
use remote::requests::generate_url;
use remote::utils::DROP_CLIENT_ASYNC;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fs::{create_dir_all, remove_file};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tokio::sync::mpsc::Sender;
use utils::{app_emit, lock, send};

use crate::downloads::utils::get_disk_available;
use crate::library::{Game, on_game_complete, push_game_update, set_partially_installed};
use crate::state::GameStatusManager;

use super::download_logic::download_game_chunk;
use super::drop_data::{DropData, InstalledFileRecord, hash_file};
use super::file_plan::{
    ConflictChoice, FileConflict, FilePlan, InstallDisk, PendingConflicts, clear_pending,
    compute_plan, file_metadata, register_pending, take_answer,
};

static RETRY_COUNT: usize = 3;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInformation {
    file_list: HashMap<String, String>,
    // Relative path -> server-known whole-file SHA-256 (hex), for files whose
    // owning version has one recorded. Missing entries (or an old server that
    // doesn't send this field at all) mean no server hash is available.
    #[serde(default)]
    file_hashes: HashMap<String, String>,
    manifests: HashMap<String, Manifest>,
    install_size: u64,
}

pub struct GameDownloadAgent {
    pub metadata: DownloadableMetadata,
    pub configuration: UserConfiguration,
    pub control_flag: DownloadThreadControl,
    pub dl_info: Mutex<Option<DownloadInformation>>,
    pub download_progress: Arc<ProgressObject>,
    pub disk_progress: Arc<ProgressObject>,
    depot_manager: Arc<DepotManager>,
    sender: Sender<DownloadManagerSignal>,
    pub dropdata: DropData,
    status: Mutex<DownloadStatus>,
    // Whether we've already thrown away our manifest and re-fetched a fresh,
    // uncached, non-delta one because a depot 404'd a chunk. Only worth doing
    // once per agent - if the content is still missing afterwards, it's really
    // gone from the depots and no amount of resyncing will conjure it up.
    resynced_manifest: Mutex<bool>,
}

impl Debug for GameDownloadAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameDownloadAgent").finish()
    }
}

impl GameDownloadAgent {
    pub async fn new(
        metadata: DownloadableMetadata,
        base_dir: PathBuf,
        sender: Sender<DownloadManagerSignal>,
        depot_manager: Arc<DepotManager>,
        configuration: UserConfiguration,
    ) -> Result<Self, ApplicationDownloadError> {
        // Don't run by default
        let control_flag = DownloadThreadControl::new(DownloadThreadControlFlag::Stop);

        let game_name = get_cached_object::<Game>(&format!("game/{}", metadata.id))
            .map(|v| v.library_path)
            .unwrap_or(metadata.id.clone());

        let base_dir_path = Path::new(&base_dir);
        info!("base dir {}", base_dir_path.display());
        let data_base_dir_path = base_dir_path.join(game_name);
        info!("data dir path {}", data_base_dir_path.display());

        create_dir_all(data_base_dir_path.clone())?;

        let stored_manifest = DropData::generate(
            metadata.id.clone(),
            metadata.version.clone(),
            metadata.target_platform,
            data_base_dir_path.clone(),
            configuration.clone(),
        );

        let result = Self {
            metadata,
            control_flag,
            dl_info: Mutex::new(None),
            download_progress: Arc::new(ProgressObject::new(
                0,
                0,
                sender.clone(),
                ProgressType::Download,
            )),
            disk_progress: Arc::new(ProgressObject::new(
                0,
                0,
                sender.clone(),
                ProgressType::Disk,
            )),
            sender,
            dropdata: stored_manifest,
            status: Mutex::new(DownloadStatus::Queued),
            depot_manager,
            configuration,
            resynced_manifest: Mutex::new(false),
        };

        result.ensure_manifest_exists().await?;
        result.resync_hashes_with_server();

        let required_space = {
            let dl_info = lock!(result.dl_info);
            let dl_info = dl_info.as_ref().unwrap();
            if result.is_untracked_update() {
                dl_info.install_size
            } else {
                estimate_write_bytes(dl_info, &result.dropdata.get_installed_files())
            }
        };

        let available_space = get_disk_available(data_base_dir_path)? as u64;

        if required_space > available_space {
            return Err(ApplicationDownloadError::DiskFull(
                required_space,
                available_space,
            ));
        }

        Ok(result)
    }

    // Blocking
    pub fn setup_download(&self, app_handle: &AppHandle) -> Result<(), ApplicationDownloadError> {
        let mut db_lock = borrow_db_mut_checked();
        let status = ApplicationTransientStatus::Downloading {
            version_id: self.metadata.version.clone(),
        };
        db_lock
            .applications
            .transient_statuses
            .insert(self.metadata(), status.clone());
        // Don't use GameStatusManager because this game isn't installed
        push_game_update(app_handle, &self.metadata().id, None, (None, Some(status)));

        if !self.check_manifest_exists() {
            return Err(ApplicationDownloadError::NotInitialized);
        }

        self.control_flag.set(DownloadThreadControlFlag::Go);

        Ok(())
    }

    // Blocking
    pub async fn download(&self, app_handle: &AppHandle) -> Result<bool, ApplicationDownloadError> {
        self.setup_download(app_handle)?;
        let timer = Instant::now();

        info!("beginning download for {}...", self.metadata().id);

        let mut res = self.run(app_handle).await;

        // A depot 404'd a chunk our manifest listed. Before giving up (and
        // leaving the user unable to ever install this game again without
        // manually wiping their client), throw away everything we're working
        // from and start over as if this were a fresh install: an uncached,
        // non-delta manifest and no locally-completed chunks.
        if matches!(res, Err(ApplicationDownloadError::ContentOutOfSync))
            && self.resync_manifest_from_scratch().await
        {
            self.setup_download(app_handle)?;
            res = self.run(app_handle).await;
        }

        debug!(
            "{} took {}ms to download",
            self.metadata.id,
            timer.elapsed().as_millis()
        );
        res
    }

    /// Recovery path for [`ApplicationDownloadError::ContentOutOfSync`]:
    /// discard the manifest we were working from, ask the server for a freshly
    /// built full one, and forget every chunk we thought we'd already
    /// completed. Returns whether the caller should re-run the download.
    ///
    /// Only ever runs once per agent, so a genuinely missing version fails
    /// after one extra attempt instead of looping.
    async fn resync_manifest_from_scratch(&self) -> bool {
        {
            let mut resynced = lock!(self.resynced_manifest);
            if *resynced {
                return false;
            }
            *resynced = true;
        }

        warn!(
            "manifest for {} is out of sync with depot content, re-fetching a full manifest and restarting the download",
            self.metadata.id
        );

        if let Err(e) = self.download_manifest(true).await {
            error!(
                "failed to re-fetch manifest for {}: {e:?}",
                self.metadata.id
            );
            return false;
        }

        // Whatever we downloaded against the old manifest can't be assumed to
        // match the new one, so re-verify/re-fetch the lot.
        self.dropdata.set_contexts(&[]);
        self.dropdata.write();

        true
    }

    pub fn check_manifest_exists(&self) -> bool {
        lock!(self.dl_info).is_some()
    }

    pub async fn ensure_manifest_exists(&self) -> Result<(), ApplicationDownloadError> {
        if lock!(self.dl_info).is_some() {
            return Ok(());
        }

        self.download_manifest(false).await
    }

    /// Fetches the download manifest for this version.
    ///
    /// `full_resync` asks the server to rebuild the manifest from scratch
    /// (bypassing its manifest cache) and drops the delta hint, so we get the
    /// complete file/chunk set for this version rather than only what changed
    /// since whatever we last had installed. That's the equivalent of a fresh
    /// client install, and is what we fall back to when the manifest we were
    /// given references content the depots don't have.
    async fn download_manifest(&self, full_resync: bool) -> Result<(), ApplicationDownloadError> {
        let client = DROP_CLIENT_ASYNC.clone();
        // With a record of what Drop installed, the client works out which
        // files changed itself (see `file_plan`), so it wants every chunk.
        // Letting the server leave out files it thinks are unchanged misses
        // any file whose content was resynced in place on the server. Only
        // installs from before files were tracked still rely on that.
        let previous = if full_resync || !self.is_untracked_update() {
            ""
        } else {
            self.dropdata
                .previously_installed_version
                .as_deref()
                .unwrap_or("")
        };
        let url = generate_url(
            &["/api/v1/client/game/manifest"],
            &[
                ("id", &self.metadata.id),
                ("version", &self.metadata.version),
                ("previous", previous),
                ("refresh", if full_resync { "true" } else { "false" }),
            ],
        )
        .map_err(ApplicationDownloadError::Communication)?;

        let response = client
            .get(url)
            .header("Authorization", generate_authorization_header())
            .send()
            .await
            .map_err(|e| ApplicationDownloadError::Communication(e.into()))?;

        if response.status() != 200 {
            return Err(ApplicationDownloadError::Communication(
                RemoteAccessError::ManifestDownloadFailed(
                    response.status(),
                    response.text().await.unwrap(),
                ),
            ));
        }

        let manifest_download: DownloadInformation = response
            .json()
            .await
            .map_err(|e| ApplicationDownloadError::Communication(e.into()))?;

        if let Ok(mut manifest) = self.dl_info.lock() {
            *manifest = Some(manifest_download);
            return Ok(());
        }

        Err(ApplicationDownloadError::Lock)
    }

    // If the server's manifest for a file we previously recorded as installed
    // now reports a different hash than what's in our local .dropdata, this
    // versionId's content was resynced/replaced server-side (the admin
    // "editable game versions" feature) while it was already installed
    // locally. Any chunks the download logic thinks are "already complete"
    // may actually be stale content, so drop the chunk-completion cache and
    // let the download pass re-verify/re-fetch everything against the fresh
    // server data instead of skip-treating chunks that predate the resync.
    fn resync_hashes_with_server(&self) {
        let changed = {
            let dl_info = lock!(self.dl_info);
            let Some(dl_info) = dl_info.as_ref() else {
                return;
            };
            let previously_installed = self.dropdata.get_installed_files();
            previously_installed.iter().any(|(path, record)| {
                matches!(
                    (&record.server_hash, dl_info.file_hashes.get(path)),
                    (Some(old_hash), Some(new_hash)) if old_hash != new_hash
                )
            })
        };

        if changed {
            info!(
                "server-side content changed for an already-installed version of {}, resyncing local chunk state",
                self.metadata.id
            );
            self.dropdata.set_contexts(&[]);
            self.dropdata.write();
        }
    }

    // Sets up progress for download writes
    fn setup_progress(&self, total_chunks: usize, download_bytes: u64, disk_bytes: u64) {
        self.download_progress
            .set_max(download_bytes.try_into().unwrap());
        self.download_progress.set_size(total_chunks);
        self.download_progress.reset();

        self.disk_progress.set_max(disk_bytes.try_into().unwrap());
        self.disk_progress.set_size(total_chunks);
        self.disk_progress.reset();
    }

    async fn run(&self, app_handle: &AppHandle) -> Result<bool, ApplicationDownloadError> {
        self.depot_manager.sync_depots().await?;
        info!("synced depots");
        let manifests_chunks: Vec<(String, HashMap<String, ChunkData>, [u8; 16])> = {
            let dl_info = lock!(self.dl_info);
            dl_info
                .as_ref()
                .unwrap()
                .manifests
                .iter()
                .map(|v| (v.0.clone(), v.1.chunks.clone(), v.1.key))
                .collect()
        };
        let (file_list, file_hashes) = {
            let dl_info = lock!(self.dl_info);
            let dl_info = dl_info.as_ref().unwrap();
            (dl_info.file_list.clone(), dl_info.file_hashes.clone())
        };

        // Work out what to write and delete, asking the player first about
        // anything they changed themselves.
        let Some(plan) = self
            .plan_files(app_handle, &manifests_chunks, &file_list, &file_hashes)
            .await?
        else {
            return Ok(false);
        };
        info!(
            "{}: writing {} file(s), deleting {}, leaving {} as they are",
            self.metadata.id,
            plan.write.len(),
            plan.delete.len(),
            plan.unchanged.len() + plan.declined.len()
        );
        self.start_plan(&plan)?;

        // Only fetch chunks that carry at least one file being written.
        let write_set: HashSet<String> = plan.write.keys().cloned().collect();
        let manifests_chunks: Vec<(String, HashMap<String, ChunkData>, [u8; 16])> =
            manifests_chunks
                .into_iter()
                .map(|(version_id, chunks, key)| {
                    let chunks = chunks
                        .into_iter()
                        .filter(|(_, chunk)| {
                            chunk.files.iter().any(|f| {
                                write_set.contains(&f.filename)
                                    && file_list.get(&f.filename) == Some(&version_id)
                            })
                        })
                        .collect::<HashMap<_, _>>();
                    (version_id, chunks, key)
                })
                .collect();
        let chunk_len = manifests_chunks.iter().map(|v| v.1.len()).sum::<usize>();
        let needed_chunk_ids: Vec<String> = manifests_chunks
            .iter()
            .flat_map(|(_, chunks, _)| chunks.keys().cloned())
            .collect();
        let (download_bytes, disk_bytes) =
            manifests_chunks
                .iter()
                .fold((0u64, 0u64), |totals, (version_id, chunks, _)| {
                    chunks.values().flat_map(|c| c.files.iter()).fold(
                        totals,
                        |(download, disk), f| {
                            let length = f.length as u64;
                            let written = write_set.contains(&f.filename)
                                && file_list.get(&f.filename) == Some(version_id);
                            (download + length, disk + if written { length } else { 0 })
                        },
                    )
                });
        self.setup_progress(chunk_len, download_bytes, disk_bytes);
        info!("setup progress objects");

        let mut completed_chunks = {
            let completed_chunks = lock!(self.dropdata.contexts);
            completed_chunks.clone()
        };
        info!("started with {} existing chunks", completed_chunks.len());
        let mut max_download_threads = borrow_db_checked().settings.max_download_threads;
        if max_download_threads == 0 {
            max_download_threads = 1;
        }

        let file_list = &file_list;
        let write_set = &write_set;
        let base_path = &self.dropdata.base_path;

        let local_completed_chunks = completed_chunks.clone();

        let mut chunk_completions = FuturesUnordered::new();

        let mut outputs = Vec::new();

        let mut handle_output =
            |value: Result<Option<String>, ApplicationDownloadError>| match value {
                Ok(value) => {
                    if let Some(chunk_id) = value {
                        outputs.push(chunk_id);
                    }
                    Ok(())
                }
                Err(err) => Err(err),
            };

        let mut index = 0;
        for (version_id, chunks, key) in manifests_chunks.into_iter() {
            let version_id = &version_id;
            for (chunk_id, chunk_data) in chunks.into_iter() {
                let download_progress_handle = ProgressHandle::new(
                    self.download_progress.get(index),
                    self.download_progress.clone(),
                );
                let disk_progress_handle =
                    ProgressHandle::new(self.disk_progress.get(index), self.disk_progress.clone());
                index += 1;

                let chunk_length = chunk_data.files.iter().map(|v| v.length).sum();

                if *local_completed_chunks.get(&chunk_id).unwrap_or(&false) {
                    download_progress_handle.skip(chunk_length);
                    continue;
                }

                // Pick a depot that has the version this chunk actually
                // belongs to, not the version we're installing - for a delta
                // version, most chunks are served out of the older versions
                // its manifest chain resolves through, and a depot that only
                // holds the newer version would 404 every one of them.
                let (depot, permit) =
                    match self.depot_manager.next_depot(&self.metadata.id, version_id) {
                        Ok(v) => v,
                        Err(err) => {
                            return Err(err.into());
                        }
                    };

                let local_version_id = version_id.clone();
                while chunk_completions.len() >= max_download_threads {
                    handle_output(
                        chunk_completions
                            .next()
                            .await
                            .expect("max download threads is zero?"),
                    )?;
                }
                chunk_completions.push(async move {
                    for i in 0..RETRY_COUNT {
                        match download_game_chunk(
                            &self.metadata.id,
                            &local_version_id,
                            &chunk_id,
                            &depot,
                            &key,
                            &chunk_data,
                            file_list,
                            write_set,
                            base_path,
                            &self.control_flag,
                            &download_progress_handle,
                            &disk_progress_handle,
                        )
                        .await
                        {
                            Ok(true) => {
                                drop(permit);
                                return Ok(Some(chunk_id.clone()));
                            }
                            Ok(false) => return Ok(None),
                            Err(e) => {
                                warn!("got error for chunk id {}: {e:?}", chunk_id);

                                // A missing chunk is deterministic - the depot
                                // will 404 it just as hard two more times.
                                // Fail out immediately so the agent can resync
                                // its manifest and start over.
                                let retry =
                                    !matches!(e, ApplicationDownloadError::ContentOutOfSync);

                                if i == RETRY_COUNT - 1 || !retry {
                                    warn!("retry logic failed, not re-attempting.");
                                    return Err(e);
                                }
                            }
                        }
                    }
                    Ok(None)
                });
            }
        }

        while let Some(value) = chunk_completions.next().await {
            handle_output(value)?
        }

        for completed_chunk in outputs {
            completed_chunks.insert(completed_chunk, true);
        }

        let drop_data_chunks = completed_chunks
            .iter()
            .map(|v| (v.0.to_string(), *v.1))
            .collect::<Vec<(String, bool)>>();

        self.dropdata.set_contexts(&drop_data_chunks);
        self.dropdata.write();

        info!("completed {} chunks", drop_data_chunks.len());

        // Every chunk we needed has to be done; stale entries from an earlier
        // attempt don't count.
        let done = needed_chunk_ids
            .iter()
            .filter(|chunk_id| *completed_chunks.get(*chunk_id).unwrap_or(&false))
            .count();
        if done != chunk_len {
            info!(
                "download agent for {} exited without completing ({}/{})",
                self.metadata.id.clone(),
                done,
                chunk_len,
            );
            return Ok(false);
        }

        // Every chunk reported complete - verify the files we wrote actually
        // match what the server expects. Per-chunk checksums above only ever
        // verify bytes in flight; they can't catch a bug in the offset/seek
        // logic that assembles chunks into files, or corruption that happens
        // after a chunk's bytes are already written to disk. Files we didn't
        // write are left out: a mismatch there is the player's own edit.
        if !self.verify_written_files(base_path, &plan).await? {
            return Ok(false);
        }

        self.finish_plan(&plan, file_list);
        Ok(true)
    }

    /// An update over an install made before Drop recorded which files it
    /// installed. There's nothing to tell the player's edits apart from
    /// Drop's by, so it falls back to the server's list of changed files.
    fn is_untracked_update(&self) -> bool {
        self.dropdata.previously_installed_version.is_some()
            && lock!(self.dropdata.installed_files).is_empty()
    }

    async fn plan_files(
        &self,
        app_handle: &AppHandle,
        manifests_chunks: &[(String, HashMap<String, ChunkData>, [u8; 16])],
        file_list: &HashMap<String, String>,
        file_hashes: &HashMap<String, String>,
    ) -> Result<Option<FilePlan>, ApplicationDownloadError> {
        if self.is_untracked_update() {
            let mut plan = FilePlan::default();
            for (version_id, chunks, _) in manifests_chunks {
                for f in chunks.values().flat_map(|c| c.files.iter()) {
                    if file_list.get(&f.filename) == Some(version_id) {
                        plan.write
                            .insert(f.filename.clone(), file_hashes.get(&f.filename).cloned());
                    }
                }
            }
            for path in file_list.keys() {
                if !plan.write.contains_key(path) {
                    plan.unchanged.insert(path.clone(), None);
                }
            }
            return Ok(Some(plan));
        }

        let records = self.dropdata.get_installed_files();
        let base_path = self.dropdata.base_path.clone();
        let (list, hashes) = (file_list.clone(), file_hashes.clone());
        let mut plan = tokio::task::spawn_blocking(move || {
            compute_plan(&records, &list, &hashes, &mut InstallDisk { base_path })
        })
        .await
        .map_err(|e| ApplicationDownloadError::IoError(Arc::new(io::Error::other(e))))?;

        if plan.conflicts.is_empty() {
            return Ok(Some(plan));
        }
        match self.wait_for_choices(app_handle, &plan.conflicts).await {
            Some(choices) => {
                plan.resolve(&choices);
                Ok(Some(plan))
            }
            None => Ok(None),
        }
    }

    /// Shows the player the files of theirs this download would change and
    /// waits for their answer. `None` if the download was paused or
    /// cancelled first.
    async fn wait_for_choices(
        &self,
        app_handle: &AppHandle,
        conflicts: &[FileConflict],
    ) -> Option<HashMap<String, ConflictChoice>> {
        let game_id = self.metadata.id.clone();
        let game_name = get_cached_object::<Game>(&format!("game/{game_id}"))
            .map(|g| g.m_name)
            .unwrap_or_else(|_| game_id.clone());
        let pending = PendingConflicts {
            meta: self.metadata.clone(),
            game_name,
            conflicts: conflicts.to_vec(),
        };
        info!(
            "{game_id}: {} file(s) the player changed would be changed or removed, waiting for their choice",
            conflicts.len()
        );
        register_pending(pending.clone());
        *lock!(self.status) = DownloadStatus::WaitingForInput;
        send!(self.sender, DownloadManagerSignal::UpdateUIQueue);
        app_emit!(app_handle, "download_file_conflicts", pending);

        loop {
            if let Some(choices) = take_answer(&game_id) {
                *lock!(self.status) = DownloadStatus::Downloading;
                send!(self.sender, DownloadManagerSignal::UpdateUIQueue);
                return Some(choices);
            }
            if self.control_flag.get() == DownloadThreadControlFlag::Stop {
                clear_pending(&game_id);
                app_emit!(app_handle, "download_file_conflicts_cleared", &game_id);
                // Back in line, so resuming asks again rather than leaving
                // the download stuck.
                *lock!(self.status) = DownloadStatus::Queued;
                send!(self.sender, DownloadManagerSignal::UpdateUIQueue);
                return None;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Deletes what the plan removes and clears out files about to be
    /// rewritten, recording each as mid-write so a pause doesn't make Drop's
    /// half-written file look like an edit by the player.
    fn start_plan(&self, plan: &FilePlan) -> Result<(), ApplicationDownloadError> {
        let base_path = &self.dropdata.base_path;
        let remove = |path: &str| match remove_file(base_path.join(path)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ApplicationDownloadError::IoError(Arc::new(e))),
        };

        {
            let mut records = lock!(self.dropdata.installed_files);
            for path in &plan.delete {
                remove(path)?;
                records.remove(path);
            }
            for path in &plan.untrack {
                records.remove(path);
            }
            for (path, target) in &plan.write {
                let record = records.entry(path.clone()).or_default();
                if record.pending_hash.is_none() {
                    // Chunks are written into the file in place, so start
                    // from nothing rather than leave a longer old file's tail.
                    remove(path)?;
                    record.pending_hash = Some(target.clone().unwrap_or_default());
                }
            }
        }
        self.dropdata.write();
        Ok(())
    }

    /// Records what's now installed, as the baseline for the next update.
    fn finish_plan(&self, plan: &FilePlan, file_list: &HashMap<String, String>) {
        {
            let mut records = lock!(self.dropdata.installed_files);
            for (path, current) in &plan.unchanged {
                let target = self.dl_info_hash(path).or_else(|| current.clone());
                let owner = file_list.get(path).cloned();
                match records.get_mut(path) {
                    Some(record) => {
                        // Still the content Drop would install (not an edit
                        // the player kept): refresh the baseline.
                        if current.is_none() || current == &target {
                            record.server_hash = target;
                            record.declined_hash = None;
                        }
                        record.owner = owner;
                    }
                    None => {
                        records.insert(
                            path.clone(),
                            InstalledFileRecord {
                                server_hash: target,
                                owner,
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            for (path, target) in &plan.declined {
                let record = records.entry(path.clone()).or_default();
                record.declined_hash = target.clone();
                record.owner = file_list.get(path).cloned();
            }
            for path in plan.write.keys() {
                if let Some(record) = records.get_mut(path) {
                    record.owner = file_list.get(path).cloned();
                }
            }
        }
        self.dropdata.write();
    }

    fn dl_info_hash(&self, path: &str) -> Option<String> {
        lock!(self.dl_info)
            .as_ref()
            .and_then(|dl_info| dl_info.file_hashes.get(path).cloned())
    }

    // See the call site in `run()` above for why this exists. Failing like a
    // chunk-checksum error (rather than a soft `Ok(false)`) reuses the app's
    // existing corrupt-download UX, and clearing just the affected chunks'
    // completion state means a retry only re-fetches what's actually broken.
    //
    // Returns whether verification actually ran to completion: `Ok(false)`
    // means the user paused/cancelled partway through, which is not an error.
    // Hashing a whole install can take minutes, so this has to honour the
    // control flag between files - otherwise the download manager's stop
    // handling gives up waiting on us and the download can only be halted by
    // killing the app.
    async fn verify_written_files(
        &self,
        base_path: &Path,
        plan: &FilePlan,
    ) -> Result<bool, ApplicationDownloadError> {
        let mut corrupted_chunk_ids: HashSet<String> = HashSet::new();
        let mut uncovered = false;

        for (relative_path, target) in &plan.write {
            if self.control_flag.get() == DownloadThreadControlFlag::Stop {
                info!(
                    "verification of {} interrupted by a stop signal",
                    self.metadata.id
                );
                self.dropdata.write();
                return Ok(false);
            }

            let file_path = base_path.join(relative_path);
            // Hashing is CPU- and IO-bound and completely synchronous, so it
            // goes on the blocking pool rather than parking an async runtime
            // worker for the duration.
            let hash_path = file_path.clone();
            let hashed = tokio::task::spawn_blocking(move || {
                hash_file(&hash_path).map(|hash| (hash, file_metadata(&hash_path)))
            })
            .await;
            let (hash, metadata) = match hashed {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    warn!("could not hash installed file {}: {e}", file_path.display());
                    continue;
                }
                Err(e) => {
                    warn!(
                        "hashing task for installed file {} failed: {e}",
                        file_path.display()
                    );
                    continue;
                }
            };

            if let Some(target) = target
                && target != &hash
            {
                warn!(
                    "installed file {} doesn't match the server's hash (expected {target}, got {hash}) - treating as a corrupted download",
                    file_path.display()
                );
                let _ = remove_file(&file_path);

                let dl_info = lock!(self.dl_info);
                let mut covered = false;
                if let Some(dl_info) = dl_info.as_ref() {
                    for manifest in dl_info.manifests.values() {
                        for (chunk_id, chunk_data) in manifest.chunks.iter() {
                            if chunk_data
                                .files
                                .iter()
                                .any(|f| &f.filename == relative_path)
                            {
                                corrupted_chunk_ids.insert(chunk_id.clone());
                                covered = true;
                            }
                        }
                    }
                }
                // Nothing we were given can rebuild this file, so the
                // manifest is out of date: a full one is fetched instead of
                // reporting success with the file missing.
                uncovered |= !covered;
                continue;
            }

            let mut records = lock!(self.dropdata.installed_files);
            let record = records.entry(relative_path.clone()).or_default();
            record.server_hash = target.clone();
            record.client_hash = Some(hash);
            record.size = metadata.map(|m| m.0);
            record.modified_ns = metadata.map(|m| m.1);
            record.pending_hash = None;
            record.declined_hash = None;
        }

        if uncovered {
            self.dropdata.set_contexts(&[]);
            self.dropdata.write();
            return Err(ApplicationDownloadError::ContentOutOfSync);
        }

        if !corrupted_chunk_ids.is_empty() {
            let mut contexts = lock!(self.dropdata.contexts);
            for chunk_id in &corrupted_chunk_ids {
                contexts.insert(chunk_id.clone(), false);
            }
            drop(contexts);
            self.dropdata.write();
            return Err(ApplicationDownloadError::Checksum);
        }

        self.dropdata.write();
        Ok(true)
    }

    #[allow(dead_code)]
    fn setup_validate(&self, app_handle: &AppHandle) {
        self.setup_progress(0, 0, 0);

        self.control_flag.set(DownloadThreadControlFlag::Go);

        let status = ApplicationTransientStatus::Validating {
            version_id: self.metadata.version.clone(),
        };

        let mut db_lock = borrow_db_mut_checked();
        db_lock
            .applications
            .transient_statuses
            .insert(self.metadata(), status.clone());
        push_game_update(app_handle, &self.metadata().id, None, (None, Some(status)));
    }

    pub fn validate(&self, _app_handle: &AppHandle) -> Result<bool, ApplicationDownloadError> {
        /*
        self.setup_validate(app_handle);

        let buckets = lock!(self.buckets);
        let contexts: Vec<DropValidateContext> = buckets
            .clone()
            .into_iter()
            .flat_map(|e| -> Vec<DropValidateContext> { e.into() })
            .collect();
        let max_download_threads = borrow_db_checked().settings.max_download_threads;

        info!("{} validation contexts", contexts.len());
        let pool = ThreadPoolBuilder::new()
            .num_threads(max_download_threads)
            .build()
            .unwrap_or_else(|_| {
                panic!("failed to build thread pool with {max_download_threads} threads")
            });

        let invalid_chunks = Arc::new(boxcar::Vec::new());
        pool.scope(|scope| {
            for (index, context) in contexts.iter().enumerate() {
                let current_progress = self.progress.get(index);
                let progress_handle = ProgressHandle::new(current_progress, self.progress.clone());
                let invalid_chunks_scoped = invalid_chunks.clone();
                let sender = self.sender.clone();

                scope.spawn(move |_| {
                    match validate_game_chunk(context, &self.control_flag, progress_handle) {
                        Ok(true) => {}
                        Ok(false) => {
                            invalid_chunks_scoped.push(context.checksum.clone());
                        }
                        Err(e) => {
                            error!("{e}");
                            send!(sender, DownloadManagerSignal::Error(e));
                        }
                    }
                });
            }
        });

        // If there are any contexts left which are false
        if !invalid_chunks.is_empty() {
            info!("validation of game id {} failed", self.id);

            for context in invalid_chunks.iter() {
                self.dropdata.set_context(context.1.clone(), false);
            }

            self.dropdata.write();

            return Ok(false);
        }
         */

        Ok(true)
    }

    pub fn cancel(&self, app_handle: &AppHandle) {
        // See docs on usage
        set_partially_installed(
            &self.metadata(),
            self.dropdata.base_path.display().to_string(),
            Some(app_handle),
            self.configuration.clone(),
        );

        self.dropdata.write();
    }
}

#[async_trait]
impl Downloadable for GameDownloadAgent {
    async fn download(&self, app_handle: &AppHandle) -> Result<bool, ApplicationDownloadError> {
        *lock!(self.status) = DownloadStatus::Downloading;
        self.download(app_handle).await
    }

    fn validate(&self, app_handle: &AppHandle) -> Result<bool, ApplicationDownloadError> {
        *lock!(self.status) = DownloadStatus::Validating;
        self.validate(app_handle)
    }

    fn dl_progress(&self) -> &Arc<ProgressObject> {
        &self.download_progress
    }

    fn disk_progress(&self) -> &Arc<ProgressObject> {
        &self.disk_progress
    }

    fn control_flag(&self) -> DownloadThreadControl {
        self.control_flag.clone()
    }

    fn metadata(&self) -> DownloadableMetadata {
        self.metadata.clone()
    }

    fn on_queued(&self, app_handle: &tauri::AppHandle) {
        *self.status.lock().unwrap() = DownloadStatus::Queued;
        let mut db_lock = borrow_db_mut_checked();
        let status = ApplicationTransientStatus::Queued {
            version_id: self.metadata.version.clone(),
        };
        db_lock
            .applications
            .transient_statuses
            .insert(self.metadata(), status.clone());
        push_game_update(app_handle, &self.metadata.id, None, (None, Some(status)));
    }

    fn on_error(&self, app_handle: &tauri::AppHandle, error: &ApplicationDownloadError) {
        *lock!(self.status) = DownloadStatus::Error;
        app_emit!(app_handle, "download_error", error.to_string());

        error!("error while managing download: {error:?}");

        let mut handle = borrow_db_mut_checked();
        handle
            .applications
            .transient_statuses
            .remove(&self.metadata());

        push_game_update(
            app_handle,
            &self.metadata.id,
            None,
            GameStatusManager::fetch_state(&self.metadata.id, &handle),
        );
    }

    async fn on_complete(&self, app_handle: &tauri::AppHandle) {
        match on_game_complete(
            &self.metadata(),
            self.configuration.clone(),
            self.dropdata.base_path.to_string_lossy().to_string(),
            app_handle,
        )
        .await
        {
            Ok(_) => {}
            Err(e) => {
                error!("could not mark game as complete: {e}");
                send!(
                    self.sender,
                    DownloadManagerSignal::Error(ApplicationDownloadError::DownloadError(e))
                );
            }
        }
    }

    fn on_cancelled(&self, app_handle: &tauri::AppHandle) {
        self.cancel(app_handle);
    }

    fn status(&self) -> DownloadStatus {
        lock!(self.status).clone()
    }
}

/// Bytes an update over a tracked install will write: every file whose
/// content differs from what Drop last installed. Ignores the player's edits
/// (which only ever make it write less), so it's an upper bound.
fn estimate_write_bytes(
    dl_info: &DownloadInformation,
    records: &HashMap<String, InstalledFileRecord>,
) -> u64 {
    let needs_write = |path: &str, owner: &str| match records.get(path) {
        None => true,
        Some(record) if record.pending_hash.is_some() => true,
        Some(record) => match (dl_info.file_hashes.get(path), record.baseline()) {
            (Some(target), Some(baseline)) => target != baseline,
            _ => record.owner.as_deref() != Some(owner),
        },
    };
    dl_info
        .manifests
        .iter()
        .flat_map(|(version_id, manifest)| {
            manifest
                .chunks
                .values()
                .flat_map(|c| c.files.iter())
                .map(move |f| (version_id, f))
        })
        .filter(|(version_id, f)| {
            dl_info.file_list.get(&f.filename) == Some(*version_id)
                && needs_write(&f.filename, version_id)
        })
        .map(|(_, f)| f.length as u64)
        .sum()
}
