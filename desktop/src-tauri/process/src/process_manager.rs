use std::{
    collections::HashMap,
    fs::{OpenOptions, create_dir_all},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    process::{Command, ExitStatus},
    sync::Arc,
    thread::spawn,
    time::{Duration, Instant, SystemTime},
};

use database::{
    ApplicationTransientStatus, Database, DownloadableMetadata, GameDownloadStatus, GameVersion,
    PendingPlaytimeSession, borrow_db_checked, borrow_db_mut_checked, db::DATA_ROOT_DIR,
    EmulatorOverrideKind, models::data::InstalledGameType, platform::Platform,
};
use dynfmt::Format;
use dynfmt::SimpleCurlyFormat;
use games::{library::push_game_update, state::GameStatusManager};
use log::{debug, info, warn};
use serde::Serialize;
use shared_child::SharedChild;
use tauri::{AppHandle, Emitter as _};
use utils::external_open::sanitize_external_command;

use crate::{
    PROCESS_MANAGER,
    error::ProcessError,
    flatpak,
    format::DropFormatArgs,
    parser::{LaunchParameters, ParsedCommand},
    process_handlers::{
        AsahiMuvmLauncher, LinuxNativeLauncher, MacLauncher, UMUCompatLauncher, UMUNativeLauncher,
        WindowsCmdLauncher, WindowsDirectLauncher, WindowsLauncher, WindowsPowershellLauncher,
    },
};

pub struct RunningProcess {
    handle: Arc<SharedChild>,
    start: SystemTime,
    manually_killed: bool,
    // Instant (monotonic, and on Linux excludes suspended time - see
    // checkpoint_running_sessions) marking the end of the last recorded
    // playtime chunk, paired with the wall-clock time at that same moment
    // so chunks can carry real started_at/ended_at timestamps.
    checkpoint: Instant,
    checkpoint_wall: chrono::DateTime<chrono::Utc>,
    // LunaTranslator and, if the game asked for one, the nested X session
    // they share. Held here purely for its lifetime: dropping the
    // RunningProcess (on exit or on kill_game) shuts both down. Never read,
    // by design - its Drop impl is the entire point of storing it.
    #[cfg(target_os = "linux")]
    #[allow(dead_code)]
    luna: Option<crate::luna::LunaSession>,
}

pub struct ProcessManager<'a> {
    current_platform: Platform,
    log_output_dir: PathBuf,
    processes: HashMap<String, RunningProcess>,
    game_launchers: Vec<(
        (Platform, Platform),
        &'a (dyn ProcessHandler + Sync + Send + 'static),
    )>,
    app_handle: AppHandle,
}

#[derive(Serialize)]
pub struct LaunchOption {
    name: String,
}

#[derive(Serialize)]
pub struct ProcessHandlerOption {
    id: String,
    name: String,
    description: String,
}

impl ProcessManager<'_> {
    pub fn new(app_handle: AppHandle) -> Self {
        let log_output_dir = DATA_ROOT_DIR.join("logs");

        ProcessManager {
            #[cfg(target_os = "windows")]
            current_platform: Platform::Windows,

            #[cfg(target_os = "macos")]
            current_platform: Platform::macOS,

            #[cfg(target_os = "linux")]
            current_platform: Platform::Linux,

            processes: HashMap::new(),
            log_output_dir,
            game_launchers: vec![
                // Current platform to target platform
                (
                    (Platform::Windows, Platform::Windows),
                    &WindowsLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
                (
                    (Platform::Windows, Platform::Windows),
                    &WindowsDirectLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
                (
                    (Platform::Windows, Platform::Windows),
                    &WindowsCmdLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
                (
                    (Platform::Windows, Platform::Windows),
                    &WindowsPowershellLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
                (
                    (Platform::Linux, Platform::Linux),
                    &LinuxNativeLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
                (
                    (Platform::Linux, Platform::Linux),
                    &UMUNativeLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
                (
                    (Platform::macOS, Platform::macOS),
                    &MacLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
                (
                    (Platform::Linux, Platform::Windows),
                    &AsahiMuvmLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
                (
                    (Platform::Linux, Platform::Windows),
                    &UMUCompatLauncher {} as &(dyn ProcessHandler + Sync + Send + 'static),
                ),
            ],
            app_handle,
        }
    }

    pub fn kill_game(&mut self, game_id: String) -> Result<(), io::Error> {
        match self.processes.get_mut(&game_id) {
            Some(process) => {
                process.manually_killed = true;

                // Games are launched through wrapper processes (umu-run -> proton ->
                // wine[server] -> the actual game). umu-run calls setsid() on Proton
                // internally, which moves the real game tree into a *new* session and
                // process group, distinct from the one we set at spawn time - so
                // signalling that group only ever reaches umu-run itself, never the
                // actual game. Find every real descendant via /proc and signal them
                // directly. We use SIGTERM first (not SIGKILL): umu-run installs its
                // own SIGTERM handler that walks and forwards to its process tree,
                // which is a second safety net on top of our own walk, but only if we
                // don't kill it outright before it gets to run.
                #[cfg(unix)]
                {
                    let root_pid = process.handle.id() as i32;

                    #[cfg(target_os = "linux")]
                    let descendants = collect_descendant_pids(root_pid);
                    #[cfg(not(target_os = "linux"))]
                    let descendants: Vec<i32> = Vec::new();

                    let mut targets = descendants;
                    targets.push(root_pid);

                    send_signal_to_all(&targets, libc::SIGTERM);
                    // Best-effort: also signal the group set at spawn time, in case
                    // the tree stayed together (e.g. non-umu launch paths).
                    unsafe {
                        libc::kill(-root_pid, libc::SIGTERM);
                    }

                    let deadline = Instant::now() + Duration::from_secs(3);
                    while Instant::now() < deadline && targets.iter().any(|&pid| pid_alive(pid)) {
                        std::thread::sleep(Duration::from_millis(100));
                    }

                    let remaining: Vec<i32> =
                        targets.into_iter().filter(|&pid| pid_alive(pid)).collect();
                    if !remaining.is_empty() {
                        warn!(
                            "processes {remaining:?} for {game_id} didn't exit after SIGTERM, sending SIGKILL"
                        );
                        send_signal_to_all(&remaining, libc::SIGKILL);
                        unsafe {
                            libc::kill(-root_pid, libc::SIGKILL);
                        }
                    }
                }

                kill_process_tree(&process.handle)?;
                let exit_status = process.handle.wait()?;
                info!("exit status: {:?}", exit_status);
                Ok(())
            }
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Game ID not running",
            )),
        }
    }

    pub fn get_log_dir(&self, game_id: String) -> PathBuf {
        self.log_output_dir.join(game_id)
    }

    fn on_process_finish(
        &mut self,
        game_id: String,
        result: Result<ExitStatus, std::io::Error>,
    ) -> Result<(), ProcessError> {
        if !self.processes.contains_key(&game_id) {
            warn!(
                "process on_finish was called, but game_id is no longer valid. finished with result: {result:?}"
            );
            return Ok(());
        }

        debug!("process for {:?} exited with {:?}", game_id, result);

        let process = match self.processes.remove(&game_id) {
            Some(process) => process,
            None => {
                info!("Attempted to stop process {game_id} which didn't exist");
                return Ok(());
            }
        };

        let mut db_handle = borrow_db_mut_checked();
        let meta = db_handle
            .applications
            .installed_game_version
            .get(&game_id)
            .cloned()
            .unwrap_or_else(|| panic!("Could not get installed version of {}", game_id));
        db_handle.applications.transient_statuses.remove(&meta);

        let current_state = db_handle.applications.game_statuses.get_mut(&game_id);
        if let Some(GameDownloadStatus::Installed { install_type, .. }) = current_state
            && let Ok(exit_code) = result
            && exit_code.success()
        {
            *install_type = InstalledGameType::Installed;
        }

        let elapsed = process.start.elapsed().unwrap_or(Duration::ZERO);
        // If we started and ended really quickly, something might've gone wrong
        // Or if the status isn't 0
        // Or if it's an error
        if !process.manually_killed
            && (elapsed.as_secs() <= 2 || result.map_or(true, |r| !r.success()))
        {
            warn!("drop detected that the game {game_id} may have failed to launch properly");
            let _ = self.app_handle.emit("launch_external_error", &game_id);
        }

        // Record a playtime session for later sync (see PlaytimeSyncer).
        // Measured from the last checkpoint (see checkpoint_running_sessions),
        // not from process.start, since checkpointing already flushed
        // everything up to that point as its own chunk(s) - counting from
        // process.start here would double-count them. Using an Instant
        // (monotonic clock) rather than SystemTime also means a system
        // suspend/resume during this final stretch isn't counted as
        // playtime: on Linux, CLOCK_MONOTONIC (what Instant is backed by)
        // excludes suspended time, unlike wall-clock time.
        //
        // The >2s threshold reuses the same "may have failed to launch"
        // cutoff just above, rather than introducing a second magic number
        // for "was this a real session" - manual kills of an actual play
        // session still flow through here normally and get recorded like
        // any other.
        let seconds = process.checkpoint.elapsed().as_secs();
        if seconds > 2 {
            let started_at = process.checkpoint_wall;
            let ended_at = chrono::Utc::now();
            db_handle
                .applications
                .pending_playtime_sessions
                .push(PendingPlaytimeSession {
                    id: uuid::Uuid::new_v4().to_string(),
                    game_id: game_id.clone(),
                    seconds,
                    started_at,
                    ended_at,
                });
        }

        let version_data = match db_handle.applications.game_versions.get(&meta.version) {
            // This unwrap here should be resolved by just making the hashmap accept an option rather than just a String
            Some(res) => res,
            None => todo!(),
        };

        let status = GameStatusManager::fetch_state(&game_id, &db_handle);

        push_game_update(
            &self.app_handle,
            &game_id,
            Some(version_data.clone()),
            status,
        );
        Ok(())
    }

    // Flushes a playtime chunk for every currently-running game, covering
    // time played since each one's last checkpoint (or launch, if this is
    // its first). Called periodically by PlaytimeCheckpointer so that a game
    // that's still running when Drop's own process dies - e.g. Steam
    // killing the whole process tree when the user hits "Stop" on a
    // non-Steam shortcut, which on_process_finish never gets a chance to
    // run for - only loses at most one checkpoint interval of playtime
    // instead of the entire session.
    pub fn checkpoint_running_sessions(&mut self) {
        const MIN_CHUNK_SECS: u64 = 2;

        let mut db_handle = borrow_db_mut_checked();
        for (game_id, process) in &mut self.processes {
            let seconds = process.checkpoint.elapsed().as_secs();
            if seconds <= MIN_CHUNK_SECS {
                continue;
            }

            let started_at = process.checkpoint_wall;
            let ended_at = chrono::Utc::now();
            db_handle
                .applications
                .pending_playtime_sessions
                .push(PendingPlaytimeSession {
                    id: uuid::Uuid::new_v4().to_string(),
                    game_id: game_id.clone(),
                    seconds,
                    started_at,
                    ended_at,
                });

            process.checkpoint = Instant::now();
            process.checkpoint_wall = ended_at;
        }
    }

    fn fetch_process_handler(
        &self,
        db_lock: &Database,
        target_platform: &Platform,
        override_id: Option<&str>,
    ) -> Result<&(dyn ProcessHandler + Send + Sync), ProcessError> {
        // An explicit override wins, as long as it's valid for the current platform.
        if let Some(override_id) = override_id
            && let Some(handler) = self.game_launchers.iter().find(|e| {
                let (e_current, e_target) = e.0;
                e_current == self.current_platform
                    && e_target == *target_platform
                    && e.1.id() == override_id
                    && e.1.valid_for_platform(db_lock, target_platform)
            })
        {
            return Ok(handler.1);
        }

        Ok(self
            .game_launchers
            .iter()
            .find(|e| {
                let (e_current, e_target) = e.0;
                e_current == self.current_platform
                    && e_target == *target_platform
                    && e.1.valid_for_platform(db_lock, target_platform)
            })
            .ok_or(ProcessError::InvalidPlatform)?
            .1)
    }

    pub fn valid_platform(&self, platform: &Platform) -> bool {
        let db_lock = borrow_db_checked();
        let process_handler = self.fetch_process_handler(&db_lock, platform, None);
        process_handler.is_ok()
    }

    pub fn get_process_handlers(
        &self,
        game_id: String,
    ) -> Result<Vec<ProcessHandlerOption>, ProcessError> {
        let db_lock = borrow_db_checked();

        let meta = db_lock
            .applications
            .installed_game_version
            .get(&game_id)
            .cloned()
            .ok_or(ProcessError::NotInstalled)?;

        let target_platform = meta.target_platform;

        let handlers = self
            .game_launchers
            .iter()
            .filter(|e| {
                let (e_current, e_target) = e.0;
                e_current == self.current_platform
                    && e_target == target_platform
                    && e.1.valid_for_platform(&db_lock, &target_platform)
            })
            .map(|e| ProcessHandlerOption {
                id: e.1.id().to_string(),
                name: e.1.name().to_string(),
                description: e.1.description().to_string(),
            })
            .collect();

        Ok(handlers)
    }

    pub fn get_launch_options(game_id: String) -> Result<Vec<LaunchOption>, ProcessError> {
        let db_lock = borrow_db_checked();

        let meta = db_lock
            .applications
            .installed_game_version
            .get(&game_id)
            .cloned()
            .ok_or(ProcessError::NotInstalled)?;

        let game_version = db_lock
            .applications
            .game_versions
            .get(&meta.version)
            .ok_or(ProcessError::InvalidVersion)?;

        let launch_options = game_version
            .launches
            .iter()
            .filter(|v| v.platform == meta.target_platform)
            .map(|v| LaunchOption {
                name: v.name.clone(),
            })
            .collect::<Vec<LaunchOption>>();

        Ok(launch_options)
    }

    pub fn launch_process(
        &mut self,
        game_id: String,
        launch_process_index: usize,
    ) -> Result<(), ProcessError> {
        if self.processes.contains_key(&game_id) {
            return Err(ProcessError::AlreadyRunning);
        }

        let mut db_lock = borrow_db_mut_checked();

        let meta = db_lock
            .applications
            .installed_game_version
            .get(&game_id)
            .cloned()
            .ok_or(ProcessError::NotInstalled)?;

        let game_status = db_lock
            .applications
            .game_statuses
            .get(&game_id)
            .ok_or(ProcessError::NotInstalled)?;

        let (version_name, install_dir) = match game_status {
            GameDownloadStatus::Installed {
                version_id: version_name,
                install_dir,
                install_type: InstalledGameType::Installed | InstalledGameType::SetupRequired,
                ..
            } => (version_name, install_dir),
            _ => return Err(ProcessError::NotInstalled),
        };

        debug!(
            "Launching process {:?} with version {:?}",
            game_id,
            db_lock.applications.game_versions.get(version_name)
        );

        let game_version = db_lock
            .applications
            .game_versions
            .get(version_name)
            .ok_or(ProcessError::InvalidVersion)?;

        #[cfg(target_os = "linux")]
        let (luna_enabled, nested_enabled) = (
            game_version.user_configuration.luna_translator,
            game_version.user_configuration.nested_session,
        );

        let game_log_folder = &self.get_log_dir(game_id);
        create_dir_all(game_log_folder)?;

        let current_time = chrono::offset::Local::now();
        let log_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .read(true)
            .create(true)
            .open(game_log_folder.join(format!(
                "{}-{}.log",
                meta.version,
                current_time.timestamp()
            )))?;

        let error_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .read(true)
            .create(true)
            .open(game_log_folder.join(format!(
                "{}-{}-error.log",
                meta.version,
                current_time.timestamp()
            )))?;

        let target_platform = meta.target_platform;

        let process_handler = self.fetch_process_handler(
            &db_lock,
            &target_platform,
            game_version.user_configuration.override_handler.as_deref(),
        )?;
        debug!("using process handler {:?}", process_handler.id());

        let (target_command, emulator, configured_working_dir) = match game_status {
            GameDownloadStatus::Installed {
                install_type: InstalledGameType::Installed,
                ..
            } => {
                let (_, launch_config) = game_version
                    .launches
                    .iter()
                    .filter(|v| v.platform == target_platform)
                    .enumerate()
                    .find(|(i, _)| *i == launch_process_index)
                    .ok_or(ProcessError::NotInstalled)?;
                (
                    launch_config.command.clone(),
                    launch_config.emulator.as_ref(),
                    launch_config.working_directory.clone(),
                )
            }
            GameDownloadStatus::Installed {
                install_type: InstalledGameType::SetupRequired,
                ..
            } => {
                let setup_config = game_version
                    .setups
                    .iter()
                    .find(|v| v.platform == target_platform)
                    .ok_or(ProcessError::NotInstalled)?;

                (
                    setup_config.command.clone(),
                    None,
                    setup_config.working_directory.clone(),
                )
            }
            _ => unreachable!("Game registered as 'Partially Installed'"),
        };

        let mut target_command = ParsedCommand::parse(target_command)?;

        // Bring-your-own-emulator mode: the user's own local install of
        // this emulator replaces the one Drop would otherwise need to have
        // downloaded from the server.
        let local_emulator = emulator
            .filter(|_| db_lock.settings.byo_emulator)
            .and_then(|emulator| {
                let emulator_override = db_lock.settings.emulator_overrides.get(&emulator.game_id)?;
                let local = emulator_override.for_version(&emulator.version_id)?;
                Some((emulator_override.name.clone(), local.clone()))
            });

        let mut working_dir_override: Option<PathBuf> = None;

        // Captured before the launch command gets wrapped in umu-run/Proton
        // (see below) or reconstructed into a shell string, since by then
        // the "command" is the wrapper's path, not the game's.
        let (target_launch_string, game_executable_path) = if let Some((emulator_name, local_emulator)) =
            local_emulator
        {
            target_command.make_absolute(PathBuf::from(install_dir.clone()));

            let mut args = shell_words::split(&local_emulator.args)
                .map_err(|e| ProcessError::InvalidArguments(e.to_string()))?;
            if args.iter().any(|v| v.contains("{rom}")) {
                args.iter_mut().for_each(|v| {
                    *v = v.replace("{rom}", &target_command.command);
                });
            } else {
                args.push(target_command.command.clone());
            }
            // The game's launch option can carry its own args for the
            // emulator on top of the ROM path. `{args}` places them, since
            // some emulators only take flags before the file.
            if let Some(pos) = args.iter().position(|v| v == "{args}") {
                args.splice(pos..=pos, target_command.args.iter().cloned());
            } else {
                args.extend(target_command.args.iter().cloned());
            }

            let exe_command = match local_emulator.kind {
                EmulatorOverrideKind::Executable => {
                    if !Path::new(&local_emulator.path).is_file() {
                        return Err(ProcessError::EmulatorOverrideMissing(
                            emulator_name,
                            local_emulator.path,
                        ));
                    }
                    ParsedCommand {
                        env: target_command.env.clone(),
                        command: local_emulator.path.clone(),
                        args,
                    }
                }
                EmulatorOverrideKind::Flatpak => {
                    let flatpak = flatpak::find_flatpak()
                        .ok_or_else(|| ProcessError::FlatpakMissing(emulator_name.clone()))?;
                    let branch = local_emulator.branch.as_deref();
                    if !flatpak::is_installed(&local_emulator.path, branch) {
                        let path = match branch {
                            Some(branch) => format!("{} ({branch})", local_emulator.path),
                            None => local_emulator.path,
                        };
                        return Err(ProcessError::EmulatorOverrideMissing(emulator_name, path));
                    }
                    // The sandbox can't see the game's files unless we let
                    // it; read-write, since plenty of emulators keep saves
                    // next to the ROM.
                    let mut flatpak_args = vec![
                        "run".to_owned(),
                        format!("--filesystem={install_dir}"),
                    ];
                    // Without one, flatpak runs whichever branch is current.
                    if let Some(branch) = branch {
                        flatpak_args.push(format!("--branch={branch}"));
                    }
                    flatpak_args.push(local_emulator.path.clone());
                    flatpak_args.extend(args);
                    // Relative paths the emulator resolves should land in
                    // the game's directory, not flatpak's.
                    working_dir_override = PathBuf::from(&target_command.command)
                        .parent()
                        .map(PathBuf::from);
                    ParsedCommand {
                        env: target_command.env.clone(),
                        command: flatpak.to_string_lossy().into_owned(),
                        args: flatpak_args,
                    }
                }
            };
            let game_executable_path = PathBuf::from(&exe_command.command);

            // The emulator is a program on this machine, so it runs
            // natively regardless of which platform the server-side
            // emulator was built for.
            let native_handler =
                self.fetch_process_handler(&db_lock, &self.current_platform, None)?;
            info!(
                "{}: using local emulator {} ({}) via {:?}",
                meta.id,
                emulator_name,
                local_emulator.path,
                native_handler.id()
            );

            (
                native_handler.create_launch_process(
                    &meta,
                    exe_command.reconstruct(),
                    game_version,
                    install_dir,
                    &db_lock,
                )?,
                game_executable_path,
            )
        } else if let Some(emulator) = emulator {
            let err = ProcessError::RequiredDependency(
                emulator.game_id.clone(),
                emulator.version_id.clone(),
            );

            let emulator_metadata = db_lock
                .applications
                .installed_game_version
                .get(&emulator.game_id)
                .ok_or(err.clone())?;

            let emulator_game_status = db_lock
                .applications
                .game_statuses
                .get(&emulator.game_id)
                .ok_or(err.clone())?;

            let emulator_install_dir = match emulator_game_status {
                GameDownloadStatus::Installed {
                    install_type: InstalledGameType::Installed,
                    install_dir,
                    ..
                } => Ok(install_dir),
                GameDownloadStatus::Installed {
                    install_type: InstalledGameType::SetupRequired,
                    ..
                } => todo!(),
                _ => Err(err.clone()),
            }?;

            let emulator_game_version = db_lock
                .applications
                .game_versions
                .get(&emulator.version_id)
                .ok_or(err.clone())?;

            let emulator_launch_config = emulator_game_version
                .launches
                .iter()
                .find(|v| v.launch_id == emulator.launch_id)
                .ok_or(err)?;

            let mut exe_command = ParsedCommand::parse(emulator_launch_config.command.clone())?;
            exe_command.env.extend(target_command.env.clone());
            exe_command.make_absolute(emulator_install_dir.into());

            target_command.make_absolute(PathBuf::from(install_dir.clone()));

            exe_command.args.iter_mut().for_each(|v| {
                *v = v.replace("{rom}", &target_command.command);
            });
            // The game's launch option can carry its own args for the
            // emulator on top of the ROM path.
            exe_command.args.extend(target_command.args.iter().cloned());

            let game_executable_path = PathBuf::from(&exe_command.command);

            (
                process_handler.create_launch_process(
                    emulator_metadata,
                    exe_command.reconstruct(),
                    emulator_game_version,
                    install_dir,
                    &db_lock,
                )?,
                game_executable_path,
            )
        } else {
            target_command.make_absolute(PathBuf::from(install_dir.clone()));
            let game_executable_path = PathBuf::from(&target_command.command);

            (
                process_handler.create_launch_process(
                    &meta,
                    target_command.reconstruct(),
                    game_version,
                    install_dir,
                    &db_lock,
                )?,
                game_executable_path,
            )
        };

        // Chunk-level downloads normally set the executable bit from the
        // server manifest (see games::downloads::download_logic), but that
        // can still leave a binary non-executable - e.g. a manifest that
        // never marked the file executable, or a repair/patch path that
        // writes the file outside the chunk-download flow. Make sure the
        // thing we're actually about to spawn is runnable regardless of how
        // it ended up on disk.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Ok(metadata) = std::fs::metadata(&game_executable_path) {
                let mut permissions = metadata.permissions();
                if permissions.mode() & 0o111 == 0 {
                    info!(
                        "{}: {} is missing its executable bit, adding it",
                        meta.id,
                        game_executable_path.display()
                    );
                    permissions.set_mode(permissions.mode() | 0o111);
                    if let Err(e) = std::fs::set_permissions(&game_executable_path, permissions) {
                        warn!(
                            "{}: failed to set executable bit on {}: {e}",
                            meta.id,
                            game_executable_path.display()
                        );
                    }
                }
            }
        }

        let mut parsed_launch = ParsedCommand::parse(target_launch_string.clone())?;
        let executable_name = parsed_launch.command.clone();
        parsed_launch.make_absolute(install_dir.into());

        let format_args = DropFormatArgs::new(
            target_launch_string,
            install_dir,
            &executable_name,
            parsed_launch.command,
            None,
        );

        let target_launch_string = SimpleCurlyFormat
            .format(
                &game_version.user_configuration.launch_template,
                &format_args,
            )
            .map_err(|e| ProcessError::FormatError(e.to_string()))?
            .to_string();

        let target_launch_string = SimpleCurlyFormat
            .format(&target_launch_string, format_args)
            .map_err(|e| ProcessError::FormatError(e.to_string()))?
            .to_string();

        // Run from the directory containing the actual binary being
        // executed (the emulator's, if there is one), not the install
        // root - games/emulators that resolve their own assets relative to
        // their own binary rather than an absolute path expect this.
        // A directory the server configured for this launch wins over both.
        let working_dir = configured_working_dir
            .and_then(|dir| resolve_working_directory(&meta.id, install_dir, &dir))
            .or(working_dir_override)
            .or_else(|| game_executable_path.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(install_dir));

        let launch_parameters =
            LaunchParameters(ParsedCommand::parse(target_launch_string)?, working_dir);

        let mut launch_parameters = launch_parameters;
        launch_parameters
            .0
            .make_command_absolute_if_local(&launch_parameters.1);

        launch_parameters.0.ensure_executable()?;

        info!(
            "launching (in {}): {:?}",
            launch_parameters.1.to_string_lossy(),
            launch_parameters.0
        );

        let mut command = {
            let mut command = Command::new(launch_parameters.0.command);
            command.args(launch_parameters.0.args);
            // Set before the launch string's own env assignments so that
            // an explicit LANG/LC_ALL in the launch template or extra env
            // vars still wins. Wine/Proton pick the Windows locale up from
            // these; umu-run's container generates the locale if the host
            // doesn't have it, native games need it installed on the host.
            #[cfg(not(target_os = "windows"))]
            if let Some(locale) = game_version
                .user_configuration
                .locale
                .as_deref()
                .filter(|v| !v.is_empty())
            {
                command.env("LANG", locale).env("LC_ALL", locale);
            }
            for parts in launch_parameters
                .0
                .env
                .into_iter()
                .map(|e| e.split("=").map(|v| v.to_string()).collect::<Vec<String>>())
            {
                if let Some(key) = parts.first()
                    && let Some(value) = parts.get(1)
                {
                    command.env(key, value);
                }
            }
            command
        };

        command
            .stderr(error_file)
            .stdout(log_file)
            .env_remove("RUST_LOG")
            // When Drop itself was launched by Steam (e.g. a non-Steam game
            // shortcut created via "Add to Steam"), it inherits
            // ENABLE_GAMESCOPE_WSI=1 from Steam's own environment. That
            // routes the game's Vulkan presentation through gamescope's
            // nested WSI layer, which has nowhere to composite to outside
            // an actual gamescope session (e.g. Desktop Mode) -- the game
            // renders frames successfully but its window is never visible.
            .env_remove("ENABLE_GAMESCOPE_WSI")
            .current_dir(launch_parameters.1);
        sanitize_external_command(&mut command);

        #[cfg(target_os = "linux")]
        {
            // Every Linux game runs inside umu-run's Steam Runtime
            // (pressure-vessel) container, which needs an explicit path to
            // a fusermount helper for its own FUSE use - unlike a normal
            // FUSE client, it won't search PATH for what's effectively a
            // setuid-root mount helper, so it fails outright with
            // "$FUSERMOUNT_PROG not set" if the caller doesn't provide one,
            // even on hosts where FUSE is otherwise working fine.
            if let Some(fusermount) = find_fusermount() {
                command.env("FUSERMOUNT_PROG", fusermount);
            }

            // Separately: if the game itself is an AppImage, it'll also try
            // to FUSE-mount itself once running inside that same
            // container - FUSE isn't reliably usable in there even when
            // the host has it, so always have it extract and run from disk
            // instead of trying to mount, rather than requiring the user
            // to add a custom launch option per game.
            if is_appimage(&game_executable_path) {
                info!(
                    "{}: launching {} as an AppImage, forcing extract-and-run",
                    meta.id,
                    game_executable_path.display()
                );
                command.env("APPIMAGE_EXTRACT_AND_RUN", "1");
            }
        }

        process_handler.modify_command(&mut command);

        // Put the launched process in its own process group so kill_game can signal
        // the whole tree (umu-run -> proton -> wine[server] -> the game) at once,
        // rather than just the directly spawned wrapper process.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }

        // Started before the game, but it doesn't have to be: the bridge
        // inside the prefix retries its connection for ~30s. If the game then
        // fails to spawn, this is dropped along with the error and takes
        // LunaTranslator (and the nested session) back down with it.
        #[cfg(target_os = "linux")]
        let luna = if luna_enabled {
            Some(start_luna_session(
                &self.app_handle,
                &db_lock,
                nested_enabled,
                game_log_folder,
                &meta,
                current_time.timestamp(),
                &mut command,
            )?)
        } else {
            None
        };

        let child = command.spawn()?;

        let launch_process_handle = Arc::new(SharedChild::new(child)?);

        db_lock
            .applications
            .transient_statuses
            .insert(meta.clone(), ApplicationTransientStatus::Running {});

        push_game_update(
            &self.app_handle,
            &meta.id,
            None,
            (None, Some(ApplicationTransientStatus::Running {})),
        );

        let wait_thread_handle = launch_process_handle.clone();
        let wait_thread_game_id = meta.clone();

        self.processes.insert(
            meta.id,
            RunningProcess {
                handle: wait_thread_handle,
                start: SystemTime::now(),
                manually_killed: false,
                checkpoint: Instant::now(),
                checkpoint_wall: chrono::Utc::now(),
                #[cfg(target_os = "linux")]
                luna,
            },
        );
        spawn(move || {
            let result: Result<ExitStatus, std::io::Error> = launch_process_handle.wait();

            PROCESS_MANAGER
                .lock()
                .on_process_finish(wait_thread_game_id.id, result)
        });
        Ok(())
    }
}

/// Starts LunaTranslator for a launch, plus the nested X session when the
/// game asked for one, and points the game's own command at that session.
#[cfg(target_os = "linux")]
fn start_luna_session(
    app_handle: &AppHandle,
    database: &Database,
    nested: bool,
    log_folder: &Path,
    meta: &DownloadableMetadata,
    timestamp: i64,
    command: &mut Command,
) -> Result<crate::luna::LunaSession, ProcessError> {
    let nested_session = if nested {
        let under_gamescope = show_main_window_under_gamescope(app_handle);
        let (width, height) = primary_monitor_size(app_handle);
        let mut session = nested_session::NestedSession::start(width, height)
            .map_err(|e| ProcessError::NestedSession(e.to_string()))?;
        // gamescope draws no cursor over the session itself; see
        // `nested_session::cursor`. Not fatal: the game still runs, just
        // with an invisible pointer.
        if under_gamescope && let Err(e) = session.draw_cursor() {
            warn!("could not draw the nested session's cursor: {e}");
        }

        command
            .env("DISPLAY", session.display())
            // Given the choice, Proton and SDL both prefer Wayland - which
            // would put the game straight back on the host compositor,
            // outside the session it's meant to share with LunaTranslator.
            .env_remove("WAYLAND_DISPLAY")
            .env("SDL_VIDEODRIVER", "x11")
            .env("PROTON_ENABLE_WAYLAND", "0");

        Some(session)
    } else {
        None
    };

    let stdout = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(log_folder.join(format!("luna-{}-{timestamp}.log", meta.version)))?;
    let stderr = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(log_folder.join(format!("luna-{}-{timestamp}-error.log", meta.version)))?;

    let display = nested_session
        .as_ref()
        .map(|session| session.display().to_owned());
    let child = crate::luna::spawn(database, display.as_deref(), stdout, stderr)?;

    Ok(crate::luna::LunaSession::new(child, nested_session))
}

/// Under gamescope, a nested session is only ever shown if Drop's own window
/// is mapped.
///
/// The session's Xwayland is a *Wayland* window on gamescope, and gamescope
/// only reports X11 windows to Steam as focusable (`determine_and_apply_focus`
/// in its steamcompmgr.cpp skips everything but `XWAYLAND` windows). Steam
/// only hands the screen to an app once gamescope has reported it; after
/// that, gamescope picks among all of that app's windows, Wayland ones
/// included - and the nested session is one of them, found by walking its
/// process tree up to Steam's `SteamLaunch AppId=` reaper.
///
/// So it's Drop's own X11 window that gets the app on screen. Launched from
/// Drop's UI that window is already up; a Steam shortcut cold-starts Drop
/// with it hidden, and the game, LunaTranslator and the whole session ran
/// behind Steam's UI with nothing ever shown.
///
/// Returns whether this is gamescope at all.
#[cfg(target_os = "linux")]
fn show_main_window_under_gamescope(app_handle: &AppHandle) -> bool {
    use tauri::Manager as _;

    if std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_none() {
        return false;
    }
    match app_handle.get_window("main") {
        Some(window) => {
            if let Err(e) = window.show() {
                warn!("could not show Drop's window for the nested session: {e}");
            }
        }
        None => warn!("no main window to show for the nested session"),
    }
    true
}

/// Size for the nested X server. Under gamescope this is the game-mode
/// output, which is what we want to fill exactly.
#[cfg(target_os = "linux")]
fn primary_monitor_size(app_handle: &AppHandle) -> (u16, u16) {
    // The Steam Deck's own panel, as the most useful guess if the monitor
    // can't be queried.
    const FALLBACK: (u16, u16) = (1280, 800);

    app_handle
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| {
            let size = monitor.size();
            (
                size.width.clamp(640, 7680) as u16,
                size.height.clamp(480, 4320) as u16,
            )
        })
        .unwrap_or(FALLBACK)
}

fn kill_process_tree(handle: &SharedChild) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        // handle.kill() only terminates the launched process (often a cmd or
        // powershell wrapper), orphaning the actual game. taskkill /T kills the
        // whole process tree.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let pid = handle.id().to_string();
        let killed = Command::new("taskkill")
            .args(["/F", "/T", "/PID", pid.as_str()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if killed {
            return Ok(());
        }
    }
    handle.kill()
}

// Walk /proc to find every live descendant of `root_pid`, regardless of which
// process group or session they end up in - needed because launchers like
// umu-run re-session (setsid) the process tree they spawn, which takes it out
// of the process group we set on the direct child at launch time.
#[cfg(target_os = "linux")]
fn collect_descendant_pids(root_pid: i32) -> Vec<i32> {
    use std::fs;

    let mut parent_of: HashMap<i32, i32> = HashMap::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            let Ok(status) = fs::read_to_string(entry.path().join("status")) else {
                continue;
            };
            let ppid = status
                .lines()
                .find_map(|line| line.strip_prefix("PPid:"))
                .and_then(|v| v.trim().parse::<i32>().ok());
            if let Some(ppid) = ppid {
                parent_of.insert(pid, ppid);
            }
        }
    }

    let mut descendants = Vec::new();
    let mut queue = vec![root_pid];
    while let Some(pid) = queue.pop() {
        for (&child, &parent) in parent_of.iter() {
            if parent == pid && !descendants.contains(&child) {
                descendants.push(child);
                queue.push(child);
            }
        }
    }
    descendants
}

#[cfg(unix)]
fn send_signal_to_all(pids: &[i32], signal: i32) {
    for &pid in pids {
        // Safety: signalling PIDs we found as descendants of our own tracked
        // child (or the child itself). ESRCH (already gone) is expected and fine.
        if unsafe { libc::kill(pid, signal) } != 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                warn!("failed to send signal {signal} to pid {pid}: {err}");
            }
        }
    }
}

// Whether a pid is still a real, running process. Zombies (already exited,
// just not yet reaped by their parent) count as dead for our purposes - the
// game itself has stopped running even if its process table entry lingers.
#[cfg(target_os = "linux")]
fn pid_alive(pid: i32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // Format is "pid (comm) state ...", and comm can itself contain ')', so
    // find the *last* ')' before reading the state field after it.
    stat.rfind(')')
        .and_then(|i| stat[i + 1..].trim_start().chars().next())
        .map(|state| state != 'Z')
        .unwrap_or(false)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn pid_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

// AppImages carry a 3-byte magic ('A', 'I', <type>) right after the
// standard ELF header. Check that first since it's authoritative, and fall
// back to the file extension for anything we fail to read (e.g. permissions).
// Turns a launch's server-configured working directory into a path inside
// the install directory. The server already refuses anything that would
// leave it, but this is checked again since the value came over the network.
// A folder that isn't there falls back to the automatic choice instead of
// failing the launch, so a typo can't make a game unplayable.
fn resolve_working_directory(game_id: &str, install_dir: &str, dir: &str) -> Option<PathBuf> {
    let relative = PathBuf::from(dir.replace('\\', "/"));
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        warn!("{game_id}: ignoring working directory {dir:?}, it isn't inside the install folder");
        return None;
    }

    let path = Path::new(install_dir).join(relative);
    if !path.is_dir() {
        warn!(
            "{game_id}: configured working directory {} doesn't exist, running from the executable's folder",
            path.display()
        );
        return None;
    }
    Some(path)
}

#[cfg(target_os = "linux")]
fn is_appimage(path: &Path) -> bool {
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut header = [0u8; 11];
        if file.read_exact(&mut header).is_ok() {
            return header[0..4] == [0x7f, b'E', b'L', b'F']
                && header[8] == b'A'
                && header[9] == b'I'
                && matches!(header[10], 1 | 2);
        }
    }

    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("appimage"))
}

// Resolves the fusermount helper's absolute path by searching PATH
// ourselves, since pressure-vessel wants it handed an explicit path via
// $FUSERMOUNT_PROG rather than doing that search itself. Prefers
// fusermount3 (libfuse3) since that's what modern FUSE tooling expects.
#[cfg(target_os = "linux")]
fn find_fusermount() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    ["fusermount3", "fusermount"].into_iter().find_map(|bin| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(bin))
            .find(|candidate| candidate.is_file())
    })
}

pub trait ProcessHandler: Send + 'static {
    fn create_launch_process(
        &self,
        meta: &DownloadableMetadata,
        launch_command: String,
        game_version: &GameVersion,
        current_dir: &str,
        database: &Database,
    ) -> Result<String, ProcessError>;

    fn valid_for_platform(&self, db: &Database, target: &Platform) -> bool;

    fn modify_command(&self, command: &mut Command);

    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
}
