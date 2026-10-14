// Runs winetricks in a game's Proton prefix as a tracked job: its output is
// read as it arrives (and still written to a log), turned into per-verb
// progress, and sent to the frontend as `winetricks/update` events. One job
// per game at a time -- verbs asked for while one runs are queued onto it
// and installed by a follow-up winetricks run -- and the game can't be
// launched until it's done (see ProcessManager::launch_process).

use std::{
    collections::{HashMap, VecDeque},
    fs::{File, OpenOptions},
    io::{Read, Write as _},
    os::unix::process::CommandExt as _,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::{Arc, LazyLock, mpsc, nonpoison::Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use database::{GameDownloadStatus, borrow_db_checked, db::DATA_ROOT_DIR};
use games::library::Game;
use log::{info, warn};
use remote::cache::get_cached_object;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _};
use tauri_plugin_notification::NotificationExt as _;
use utils::external_open::sanitize_external_command;

use crate::{
    error::ProcessError,
    process_manager::{collect_descendant_pids, pid_alive, send_signal_to_all},
};

const UPDATE_EVENT: &str = "winetricks/update";
const REMOVED_EVENT: &str = "winetricks/removed";
// Lines of output kept for the frontend; the log file has the rest
const OUTPUT_TAIL: usize = 8;
// Progress-only updates (download percentages, output lines) are sent at
// most this often; verb state changes go out straight away
const EMIT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerbState {
    Waiting,
    Running,
    Installed,
    AlreadyInstalled,
    Failed,
    NotRun,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VerbProgress {
    pub name: String,
    pub state: VerbState,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    // What winetricks is doing for this verb right now, e.g.
    // "Downloading vc_redist.x64.exe"
    pub activity: Option<String>,
    pub percent: Option<u8>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WinetricksJob {
    pub game_id: String,
    pub state: JobState,
    pub verbs: Vec<VerbProgress>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    // Set while no verb is running yet (umu/Proton starting up)
    pub activity: Option<String>,
    pub output: VecDeque<String>,
    pub error: Option<String>,
    pub log_path: String,
}

struct Entry {
    job: WinetricksJob,
    pid: Option<i32>,
    cancelled: bool,
}

static JOBS: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn is_running(game_id: &str) -> bool {
    JOBS.lock()
        .get(game_id)
        .is_some_and(|entry| entry.job.state == JobState::Running)
}

pub fn jobs() -> Vec<WinetricksJob> {
    JOBS.lock()
        .values()
        .map(|entry| entry.job.clone())
        .collect()
}

pub fn log_path(game_id: &str) -> Option<PathBuf> {
    JOBS.lock()
        .get(game_id)
        .map(|entry| PathBuf::from(&entry.job.log_path))
}

// Forgets a finished job, so its result stops being shown
pub fn dismiss(app: &AppHandle, game_id: &str) {
    let mut jobs = JOBS.lock();
    if jobs
        .get(game_id)
        .is_some_and(|entry| entry.job.state != JobState::Running)
    {
        jobs.remove(game_id);
        drop(jobs);
        let _ = app.emit(REMOVED_EVENT, game_id);
    }
}

// Builds the command for a Wine helper tool (winetricks, winecfg, ...) run
// inside the same Proton/Wine environment (PROTONPATH, WINEPREFIX, GAMEID) a
// game itself launches with, via umu-run, mirroring UMUCompatLauncher's env
// construction. The caller decides where its output goes.
pub fn wine_tool_command(
    game_id: &str,
    tool: &str,
    args: &[&str],
) -> Result<Command, ProcessError> {
    let db_lock = borrow_db_checked();

    let meta = db_lock
        .applications
        .installed_game_version
        .get(game_id)
        .cloned()
        .ok_or(ProcessError::NotInstalled)?;

    let version_id = match db_lock.applications.game_statuses.get(game_id) {
        Some(GameDownloadStatus::Installed { version_id, .. }) => version_id.clone(),
        _ => return Err(ProcessError::NotInstalled),
    };

    let game_version = db_lock
        .applications
        .game_versions
        .get(&version_id)
        .ok_or(ProcessError::InvalidVersion)?;

    let proton_path = game_version
        .user_configuration
        .override_proton_path
        .clone()
        .or_else(|| db_lock.applications.default_proton_path.clone())
        .ok_or(ProcessError::NoCompat)?;

    let umu_id_override = game_version
        .launches
        .iter()
        .find(|v| v.platform == meta.target_platform)
        .and_then(|v| v.umu_id_override.as_ref())
        .map_or("", |v| v);
    let umu_game_id = if umu_id_override.is_empty() {
        game_version.version_id.clone()
    } else {
        umu_id_override.to_string()
    };

    let compat_env = crate::process_handlers::compat_env_vars(&game_version.user_configuration);

    drop(db_lock);

    let pfx_dir = DATA_ROOT_DIR.join("pfx").join(game_id);
    if !pfx_dir.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "This game hasn't been launched through Proton yet, so it has no prefix",
        )
        .into());
    }

    let umu_launcher = ::client::compat::UMU_LAUNCHER_EXECUTABLE
        .as_ref()
        .ok_or(ProcessError::NoCompat)?;

    let mut command = Command::new(umu_launcher);
    command
        .arg(tool)
        .args(args)
        .envs(compat_env)
        .env("GAMEID", umu_game_id)
        .env("PROTONPATH", proton_path)
        .env("WINEPREFIX", pfx_dir);
    sanitize_external_command(&mut command);
    Ok(command)
}

// A new log file for a run of `tool`, next to the game's own logs
pub fn new_tool_log(game_id: &str, tool: &str) -> Result<(PathBuf, File), ProcessError> {
    let log_dir = DATA_ROOT_DIR.join("logs").join(game_id);
    std::fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join(format!(
        "{tool}-{}.log",
        chrono::offset::Local::now().timestamp()
    ));
    let log_file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(&log_path)?;
    Ok((log_path, log_file))
}

// Verbs winetricks has recorded as installed in this game's prefix. It logs
// each one to winetricks.log in the Wine prefix, which Proton keeps in a pfx
// folder inside WINEPREFIX, so both places are checked. Empty when the game
// hasn't been launched yet.
pub fn installed_verbs(game_id: &str) -> Vec<String> {
    let pfx_dir = DATA_ROOT_DIR.join("pfx").join(game_id);
    [
        pfx_dir.join("winetricks.log"),
        pfx_dir.join("pfx").join("winetricks.log"),
    ]
    .iter()
    .filter_map(|path| std::fs::read_to_string(path).ok())
    .flat_map(|log| {
        log.lines()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
    })
    .collect()
}

fn waiting_verb(name: &str) -> VerbProgress {
    VerbProgress {
        name: name.to_string(),
        state: VerbState::Waiting,
        started_at: None,
        finished_at: None,
        activity: None,
        percent: None,
    }
}

// Installs `verbs` into the game's prefix, or queues them onto the job
// already running there. Returns once the run has started; progress arrives
// as events.
pub fn install(app: AppHandle, game_id: String, verbs: Vec<String>) -> Result<(), ProcessError> {
    if queue_onto_running(&app, &game_id, &verbs) {
        return Ok(());
    }

    // Built up front so a missing prefix or Proton is reported to the
    // caller rather than as a failed job
    let mut args = vec!["-q"];
    args.extend(verbs.iter().map(String::as_str));
    let command = wine_tool_command(&game_id, "winetricks", &args)?;
    let (log_path, log_file) = new_tool_log(&game_id, "winetricks")?;

    {
        let mut jobs = JOBS.lock();
        if jobs
            .get(&game_id)
            .is_some_and(|entry| entry.job.state == JobState::Running)
        {
            // Another call started one in the meantime
            drop(jobs);
            queue_onto_running(&app, &game_id, &verbs);
            return Ok(());
        }
        let mut deduped: Vec<VerbProgress> = Vec::new();
        for verb in &verbs {
            if !deduped.iter().any(|v| &v.name == verb) {
                deduped.push(waiting_verb(verb));
            }
        }
        jobs.insert(
            game_id.clone(),
            Entry {
                job: WinetricksJob {
                    game_id: game_id.clone(),
                    state: JobState::Running,
                    verbs: deduped,
                    started_at: now_ms(),
                    finished_at: None,
                    activity: Some("Starting Proton".to_string()),
                    output: VecDeque::new(),
                    error: None,
                    log_path: log_path.display().to_string(),
                },
                pid: None,
                cancelled: false,
            },
        );
    }
    info!(
        "running winetricks {verbs:?} for {game_id}, logging to {}",
        log_path.display()
    );
    emit_job(&app, &game_id);

    let log_file = Arc::new(Mutex::new(log_file));
    std::thread::Builder::new()
        .name(format!("winetricks-{game_id}"))
        .spawn(move || supervise(app, game_id, command, verbs, log_file))?;
    Ok(())
}

// Adds the verbs the running job doesn't already have to its queue. False
// when nothing is running for this game.
fn queue_onto_running(app: &AppHandle, game_id: &str, verbs: &[String]) -> bool {
    let mut jobs = JOBS.lock();
    let Some(entry) = jobs.get_mut(game_id) else {
        return false;
    };
    if entry.job.state != JobState::Running {
        return false;
    }
    for verb in verbs {
        if !entry.job.verbs.iter().any(|v| &v.name == verb) {
            entry.job.verbs.push(waiting_verb(verb));
        }
    }
    drop(jobs);
    emit_job(app, game_id);
    true
}

// Stops the running job. Verbs it hadn't finished are marked as not run.
pub fn cancel(game_id: &str) {
    let pid = {
        let mut jobs = JOBS.lock();
        let Some(entry) = jobs.get_mut(game_id) else {
            return;
        };
        if entry.job.state != JobState::Running {
            return;
        }
        entry.cancelled = true;
        entry.pid
    };
    let Some(root_pid) = pid else {
        return;
    };
    info!("cancelling winetricks for {game_id}");

    // Same approach as killing a game: umu-run re-sessions its tree, so walk
    // /proc for descendants rather than relying on the process group alone
    std::thread::spawn(move || {
        let mut targets = collect_descendant_pids(root_pid);
        targets.push(root_pid);
        send_signal_to_all(&targets, libc::SIGTERM);
        unsafe {
            libc::kill(-root_pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && targets.iter().any(|&pid| pid_alive(pid)) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let remaining: Vec<i32> = targets.into_iter().filter(|&pid| pid_alive(pid)).collect();
        if !remaining.is_empty() {
            send_signal_to_all(&remaining, libc::SIGKILL);
            unsafe {
                libc::kill(-root_pid, libc::SIGKILL);
            }
        }
    });
}

fn emit_job(app: &AppHandle, game_id: &str) {
    let job = JOBS.lock().get(game_id).map(|entry| entry.job.clone());
    if let Some(job) = job {
        let _ = app.emit(UPDATE_EVENT, job);
    }
}

enum Message {
    Line(String),
    Exited(std::io::Result<ExitStatus>),
}

enum BatchEnd {
    Done,
    Failed(String),
    Cancelled,
}

fn supervise(
    app: AppHandle,
    game_id: String,
    first_command: Command,
    first_batch: Vec<String>,
    log_file: Arc<Mutex<File>>,
) {
    let mut command = first_command;
    let mut batch = first_batch;
    let end = loop {
        let end = run_batch(&app, &game_id, command, &batch, &log_file);
        if !matches!(end, BatchEnd::Done) {
            break end;
        }

        // Verbs queued while that run was going
        batch = JOBS
            .lock()
            .get(&game_id)
            .map(|entry| {
                entry
                    .job
                    .verbs
                    .iter()
                    .filter(|v| v.state == VerbState::Waiting)
                    .map(|v| v.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        if batch.is_empty() {
            break BatchEnd::Done;
        }
        if JOBS
            .lock()
            .get(&game_id)
            .is_none_or(|entry| entry.cancelled)
        {
            break BatchEnd::Cancelled;
        }
        let mut args = vec!["-q"];
        args.extend(batch.iter().map(String::as_str));
        command = match wine_tool_command(&game_id, "winetricks", &args) {
            Ok(command) => command,
            Err(e) => break BatchEnd::Failed(e.to_string()),
        };
    };

    let finished = {
        let mut jobs = JOBS.lock();
        let Some(entry) = jobs.get_mut(&game_id) else {
            return;
        };
        let job = &mut entry.job;
        for verb in job.verbs.iter_mut() {
            if matches!(verb.state, VerbState::Waiting | VerbState::Running) {
                verb.state = VerbState::NotRun;
                verb.activity = None;
                verb.percent = None;
            }
        }
        job.activity = None;
        job.finished_at = Some(now_ms());
        job.state = match end {
            BatchEnd::Done => JobState::Succeeded,
            BatchEnd::Failed(error) => {
                job.error = Some(error);
                JobState::Failed
            }
            BatchEnd::Cancelled => JobState::Cancelled,
        };
        entry.pid = None;
        job.clone()
    };
    info!("winetricks for {game_id} finished: {:?}", finished.state);
    let _ = app.emit(UPDATE_EVENT, &finished);
    notify_if_hidden(&app, &finished);
}

fn run_batch(
    app: &AppHandle,
    game_id: &str,
    mut command: Command,
    batch: &[String],
    log_file: &Arc<Mutex<File>>,
) -> BatchEnd {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return BatchEnd::Failed(format!("Could not start winetricks: {e}")),
    };
    {
        let mut jobs = JOBS.lock();
        if let Some(entry) = jobs.get_mut(game_id) {
            entry.pid = Some(child.id() as i32);
            if entry.cancelled {
                drop(jobs);
                // Cancelled before there was anything to kill
                cancel(game_id);
            }
        }
    }

    let (sender, receiver) = mpsc::channel();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let sender = sender.clone();
        let log_file = log_file.clone();
        std::thread::spawn(move || read_lines(stream, sender, log_file));
    }
    // Waited for on its own, rather than by the readers hitting EOF: a
    // wineserver started during the run inherits the pipes and outlives it
    std::thread::spawn(move || {
        let _ = sender.send(Message::Exited(child.wait()));
    });

    let mut parser = OutputParser::default();
    let mut last_emit = Instant::now();
    let mut dirty = false;
    let status = loop {
        match receiver.recv_timeout(EMIT_INTERVAL) {
            Ok(Message::Line(line)) => {
                let mut jobs = JOBS.lock();
                if let Some(entry) = jobs.get_mut(game_id) {
                    let changed = parser.handle(&mut entry.job, batch, &line);
                    drop(jobs);
                    if changed {
                        emit_job(app, game_id);
                        last_emit = Instant::now();
                        dirty = false;
                    } else {
                        dirty = true;
                    }
                }
            }
            Ok(Message::Exited(status)) => break status,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(std::io::Error::other("lost track of winetricks"));
            }
        }
        if dirty && last_emit.elapsed() >= EMIT_INTERVAL {
            emit_job(app, game_id);
            last_emit = Instant::now();
            dirty = false;
        }
    };
    // Whatever the readers still had buffered when it exited
    while let Ok(Message::Line(line)) = receiver.recv_timeout(Duration::from_millis(200)) {
        let mut jobs = JOBS.lock();
        if let Some(entry) = jobs.get_mut(game_id) {
            parser.handle(&mut entry.job, batch, &line);
        }
    }

    // Proton's winetricks wrapper doesn't pass winetricks' exit code on, so
    // whether a verb went in is decided by winetricks.log, which winetricks
    // only appends a verb to once it's installed.
    let installed = installed_verbs(game_id);
    let mut jobs = JOBS.lock();
    let Some(entry) = jobs.get_mut(game_id) else {
        return BatchEnd::Cancelled;
    };
    entry.pid = None;
    let cancelled = entry.cancelled;
    let now = now_ms();
    let mut failed: Option<String> = None;
    for verb in entry
        .job
        .verbs
        .iter_mut()
        .filter(|v| batch.contains(&v.name))
    {
        match verb.state {
            VerbState::Running | VerbState::Waiting if installed.contains(&verb.name) => {
                verb.state = VerbState::Installed;
                verb.started_at.get_or_insert(now);
                verb.finished_at = Some(now);
            }
            VerbState::Running => {
                verb.state = if cancelled {
                    VerbState::NotRun
                } else {
                    VerbState::Failed
                };
                verb.finished_at = Some(now);
                failed.get_or_insert(verb.name.clone());
            }
            // The first verb winetricks didn't get to, when it stopped
            // without being cancelled: a failure before it got as far as
            // the verb's install (Proton not starting, a conflicting verb)
            VerbState::Waiting if failed.is_none() && !cancelled => {
                verb.state = VerbState::Failed;
                failed = Some(verb.name.clone());
            }
            VerbState::Waiting => verb.state = VerbState::NotRun,
            _ => {}
        }
        verb.activity = None;
        verb.percent = None;
    }
    drop(jobs);

    if cancelled {
        return BatchEnd::Cancelled;
    }
    match (failed, status) {
        (None, Ok(status)) => {
            if !status.success() {
                warn!("winetricks exited with {status}, but installed everything asked for");
            }
            BatchEnd::Done
        }
        (None, Err(e)) => BatchEnd::Failed(e.to_string()),
        (Some(verb), Ok(status)) => BatchEnd::Failed(match status.code() {
            Some(code) if code != 0 => format!("{verb} failed: winetricks exited with code {code}"),
            _ => format!("{verb} failed: winetricks didn't install it"),
        }),
        (Some(verb), Err(e)) => BatchEnd::Failed(format!("{verb} failed: {e}")),
    }
}

// Splits a stream into lines on \n and \r (download progress redraws one
// line with \r), copying it verbatim into the log
fn read_lines(
    mut stream: Box<dyn Read + Send>,
    sender: mpsc::Sender<Message>,
    log_file: Arc<Mutex<File>>,
) {
    let mut buffer = [0u8; 8192];
    let mut line = Vec::new();
    // Once the run is over, anything still writing here (a lingering
    // wineserver) only goes to the log. Reading on rather than closing the
    // pipe keeps its writes from failing.
    let mut listening = true;
    loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let _ = log_file.lock().write_all(&buffer[..read]);
        if !listening {
            continue;
        }
        for &byte in &buffer[..read] {
            if byte == b'\n' || byte == b'\r' {
                if !line.is_empty() {
                    let text = String::from_utf8_lossy(&line).into_owned();
                    line.clear();
                    if sender.send(Message::Line(text)).is_err() {
                        listening = false;
                        break;
                    }
                }
            } else {
                line.push(byte);
            }
        }
    }
    if listening && !line.is_empty() {
        let _ = sender.send(Message::Line(String::from_utf8_lossy(&line).into_owned()));
    }
}

#[derive(Default)]
struct OutputParser {
    downloading: bool,
    last_was_progress: bool,
}

impl OutputParser {
    // Updates the job from one line of winetricks output. True when a verb
    // changed state, which is worth telling the frontend about right away.
    fn handle(&mut self, job: &mut WinetricksJob, batch: &[String], line: &str) -> bool {
        let line = line.trim();
        if line.is_empty() {
            return false;
        }
        let percent = if self.downloading {
            download_percent(line)
        } else {
            None
        };

        // Wine's fixme spam stays in the log only
        if !line.contains(":fixme:") {
            let shown: String = line.chars().take(300).collect();
            if percent.is_some() && self.last_was_progress {
                job.output.pop_back();
            }
            job.output.push_back(shown);
            while job.output.len() > OUTPUT_TAIL {
                job.output.pop_front();
            }
            self.last_was_progress = percent.is_some();
        }

        let now = now_ms();
        let running = job
            .verbs
            .iter()
            .position(|v| v.state == VerbState::Running && batch.contains(&v.name));

        if let Some(verb) = line.strip_prefix("Executing load_") {
            self.downloading = false;
            let verb = verb.split_whitespace().next().unwrap_or_default();
            if let Some(index) = job.verbs.iter().position(|v| {
                v.name == verb && v.state == VerbState::Waiting && batch.contains(&v.name)
            }) {
                if let Some(previous) = running {
                    let previous = &mut job.verbs[previous];
                    previous.state = VerbState::Installed;
                    previous.finished_at = Some(now);
                    previous.activity = None;
                    previous.percent = None;
                }
                let verb = &mut job.verbs[index];
                verb.state = VerbState::Running;
                verb.started_at = Some(now);
                verb.activity = None;
                verb.percent = None;
                job.activity = None;
                return true;
            }
            // A dependency of the verb being installed
            if let Some(running) = running {
                job.verbs[running].activity = Some(format!("Installing dependency {verb}"));
                job.verbs[running].percent = None;
            }
            return false;
        }

        if let Some(verb) = line.strip_suffix(" already installed, skipping") {
            if let Some(found) = job.verbs.iter_mut().find(|v| {
                v.name == verb && v.state == VerbState::Waiting && batch.contains(&v.name)
            }) {
                found.state = VerbState::AlreadyInstalled;
                found.finished_at = Some(now);
                return true;
            }
            return false;
        }

        let Some(running) = running else {
            return false;
        };
        let verb = &mut job.verbs[running];

        if let Some(rest) = line.strip_prefix("Downloading ")
            && let Some((url, _)) = rest.split_once(" to ")
        {
            self.downloading = true;
            let file = url
                .split(['?', '#'])
                .next()
                .unwrap_or(url)
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(url);
            verb.activity = Some(format!("Downloading {file}"));
            verb.percent = None;
        } else if let Some(command) = line.strip_prefix("Executing ") {
            self.downloading = false;
            if let Some(activity) = describe_command(command) {
                verb.activity = Some(activity);
                verb.percent = None;
            }
        } else if let Some(percent) = percent {
            verb.percent = Some(percent);
        }
        false
    }
}

// A short description of a command winetricks runs (`Executing ...`), for
// the commands worth naming
fn describe_command(command: &str) -> Option<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let program = tokens.first()?.rsplit('/').next()?;
    if matches!(program, "cabextract" | "7z" | "7za" | "unzip" | "tar") {
        return Some("Extracting files".to_string());
    }
    let installer = tokens.iter().find(|token| {
        let lower = token.to_ascii_lowercase();
        lower.ends_with(".exe") || lower.ends_with(".msi")
    })?;
    let name = installer
        .trim_matches(['"', '\''])
        .rsplit(['/', '\\'])
        .next()?;
    Some(format!("Running {name}"))
}

// The percentage on a downloader's progress line: wget's dot lines and
// aria2c's summaries carry an `NN%`, curl's progress table a bare number in
// its first column
fn download_percent(line: &str) -> Option<u8> {
    let bytes = line.as_bytes();
    let mut found = None;
    for (index, _) in line.match_indices('%') {
        let start = bytes[..index]
            .iter()
            .rposition(|b| !(b.is_ascii_digit() || *b == b'.'))
            .map_or(0, |p| p + 1);
        if let Ok(value) = line[start..index].parse::<f32>()
            && (0.0..=100.0).contains(&value)
        {
            found = Some(value as u8);
        }
    }
    if found.is_some() {
        return found;
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() >= 12
        && let Ok(total) = tokens[0].parse::<u8>()
        && total <= 100
        && tokens[2].parse::<u8>().is_ok()
    {
        return Some(total);
    }
    None
}

// Someone who closed Drop to the tray while it ran wouldn't see the result
// in the app
fn notify_if_hidden(app: &AppHandle, job: &WinetricksJob) {
    let visible = app
        .get_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if visible || job.state == JobState::Cancelled {
        return;
    }
    let game_name = get_cached_object::<Game>(&format!("game/{}", job.game_id))
        .map(|g| g.m_name)
        .unwrap_or_else(|_| job.game_id.clone());
    let (title, body) = match job.state {
        JobState::Succeeded => (
            format!("{game_name} is ready to play"),
            format!(
                "Winetricks installed {}",
                job.verbs
                    .iter()
                    .map(|v| v.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        _ => (
            format!("Winetricks failed for {game_name}"),
            job.error.clone().unwrap_or_default(),
        ),
    };
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        warn!("couldn't show winetricks notification: {e}");
    }
}
