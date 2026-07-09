use std::{
    collections::HashMap,
    fs::{OpenOptions, create_dir_all},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    sync::Arc,
    thread::spawn,
    time::{Duration, Instant, SystemTime},
};

use database::{
    ApplicationTransientStatus, Database, DownloadableMetadata, GameDownloadStatus, GameVersion,
    borrow_db_checked, borrow_db_mut_checked, db::DATA_ROOT_DIR, models::data::InstalledGameType,
    platform::Platform,
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

        debug!("process for {:?} exited with {:?}", &game_id, result);

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
            .unwrap_or_else(|| panic!("Could not get installed version of {}", &game_id));
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
            &game_id,
            db_lock.applications.game_versions.get(version_name)
        );

        let game_version = db_lock
            .applications
            .game_versions
            .get(version_name)
            .ok_or(ProcessError::InvalidVersion)?;

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
                &meta.version,
                current_time.timestamp()
            )))?;

        let error_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .read(true)
            .create(true)
            .open(game_log_folder.join(format!(
                "{}-{}-error.log",
                &meta.version,
                current_time.timestamp()
            )))?;

        let target_platform = meta.target_platform;

        let process_handler = self.fetch_process_handler(
            &db_lock,
            &target_platform,
            game_version.user_configuration.override_handler.as_deref(),
        )?;
        debug!("using process handler {:?}", process_handler.id());

        let (target_command, emulator) = match game_status {
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

                (setup_config.command.clone(), None)
            }
            _ => unreachable!("Game registered as 'Partially Installed'"),
        };

        let mut target_command = ParsedCommand::parse(target_command)?;

        // Captured before the launch command gets wrapped in umu-run/Proton
        // (see below) or reconstructed into a shell string, since by then
        // the "command" is the wrapper's path, not the game's.
        #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
        let (target_launch_string, game_executable_path) = if let Some(emulator) = emulator {
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
            let game_executable_path = PathBuf::from(install_dir).join(&target_command.command);

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

        let launch_parameters = LaunchParameters(
            ParsedCommand::parse(target_launch_string)?,
            install_dir.into(),
        );

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
