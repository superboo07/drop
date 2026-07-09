#![deny(unused_must_use)]
#![feature(nonpoison_mutex)]
#![feature(sync_nonpoison)]
#![deny(clippy::all)]

use std::{
    env,
    fs::File,
    io::Write,
    panic::PanicHookInfo,
    path::Path,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        nonpoison::Mutex,
    },
    time::SystemTime,
};

use ::client::{
    app_state::{AppState, UmuState},
    app_status::AppStatus,
    autostart::sync_autostart_on_startup,
    compat::UMU_LAUNCHER_EXECUTABLE,
};
use ::download_manager::DownloadManagerWrapper;
use ::games::scan::scan_install_dirs;
use ::process::ProcessManagerWrapper;
use ::remote::{
    auth::{self, HandshakeRequestBody, HandshakeResponse, generate_authorization_header},
    cache::clear_cached_object,
    error::RemoteAccessError,
    fetch_object::fetch_object_wrapper,
    requests::REQUEST_TIMEOUT,
    server_proto::handle_server_proto_wrapper,
    utils::{DROP_APP_HANDLE, DROP_CLIENT_ASYNC},
};
use database::{
    DB, GameDownloadStatus, borrow_db_checked, borrow_db_mut_checked, db::DATA_ROOT_DIR,
    models::data::InstalledGameType,
};
use log::{LevelFilter, debug, info, warn};
use log4rs::{
    Config,
    append::{console::ConsoleAppender, file::FileAppender},
    config::{Appender, Root},
    encode::pattern::PatternEncoder,
};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, RunEvent, WebviewBuilder, WebviewUrl,
    WindowBuilder, WindowEvent,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::DialogExt;
use url::Url;
use utils::app_emit;

mod client;
mod collections;
mod download_manager;
mod downloads;
mod games;
mod process;
mod remote;
mod scheduler;
mod settings;
mod updates;

use client::*;
use download_manager::*;
use downloads::*;
use games::*;
use process::*;
use remote::*;
use settings::*;

use crate::scheduler::scheduler_task;

async fn setup(handle: AppHandle) -> AppState {
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d} | {l} | {f}:{L} - {m}{n}",
        )))
        .append(false)
        .build(DATA_ROOT_DIR.join("./drop.log"))
        .expect("Failed to setup logfile");

    let console = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d} | {h({l})} | {f}:{L} - {m}{n}",
        )))
        .build();

    let log_level = env::var("RUST_LOG").unwrap_or(String::from("Info"));

    let config = Config::builder()
        .appenders(vec![
            Appender::builder().build("logfile", Box::new(logfile)),
            Appender::builder().build("console", Box::new(console)),
        ])
        .build(
            Root::builder()
                .appenders(vec!["logfile", "console"])
                .build(LevelFilter::from_str(&log_level).expect("Invalid log level")),
        )
        .expect("Failed to build config");

    log4rs::init_config(config).expect("Failed to initialise log4rs");

    ProcessManagerWrapper::init(handle.clone());
    DownloadManagerWrapper::init(handle.clone());

    debug!("checking if database is set up");
    let is_set_up = DB.database_is_set_up();

    #[cfg(not(target_os = "linux"))]
    let umu_state = UmuState::NotNeeded;

    #[cfg(target_os = "linux")]
    let umu_state = match UMU_LAUNCHER_EXECUTABLE.is_some() {
        true => UmuState::Installed,
        false => UmuState::NotInstalled,
    };

    scan_install_dirs();

    if !is_set_up {
        return AppState {
            status: AppStatus::NotConfigured,
            user: None,
            umu_state,
        };
    }

    debug!("database is set up");

    // TODO: Account for possible failure
    let (app_status, user) = auth::setup().await;

    let db_handle = borrow_db_checked();
    let mut missing_games = Vec::new();
    let statuses = db_handle.applications.game_statuses.clone();
    drop(db_handle);

    for (game_id, status) in statuses {
        match status {
            GameDownloadStatus::Remote {} => {}
            GameDownloadStatus::Installed { install_dir, .. } => {
                let install_dir_path = Path::new(&install_dir);
                if !install_dir_path.exists() {
                    missing_games.push(game_id);
                }
            }
        }
    }

    info!("detected games missing: {missing_games:?}");

    let mut db_handle = borrow_db_mut_checked();
    for game_id in missing_games {
        db_handle
            .applications
            .game_statuses
            .entry(game_id)
            .and_modify(|v| *v = GameDownloadStatus::Remote {});
    }

    drop(db_handle);

    debug!("finished setup!");

    // Sync autostart state
    if let Err(e) = sync_autostart_on_startup(&handle) {
        warn!("failed to sync autostart state: {e}");
    }

    AppState {
        status: app_status,
        user,
        umu_state,
    }
}

pub fn custom_panic_handler(e: &PanicHookInfo) -> Option<()> {
    let crash_file = DATA_ROOT_DIR.join(format!(
        "crash-{}.log",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?
            .as_secs()
    ));
    let mut file = File::create_new(crash_file).ok()?;
    file.write_all(format!("Drop crashed with the following panic:\n{e}").as_bytes())
        .ok()?;
    drop(file);

    Some(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // let global_span = span!(Level::TRACE, "global_span");
    // let _enter = global_span.enter();
    std::panic::set_hook(Box::new(|e| {
        let _ = custom_panic_handler(e);
        println!("{e}");
    }));

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init());

    #[cfg(desktop)]
    #[allow(unused_variables)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|_app, argv, _cwd| {
            // when defining deep link schemes at runtime, you must also check `argv` here
        }));
    }

    let app = builder
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![
            // Core utils
            fetch_state,
            quit,
            fetch_system_data,
            open_fs,
            log_frontend,
            // User utils
            update_settings,
            fetch_settings,
            // Auth
            auth_initiate,
            auth_initiate_code,
            retry_connect,
            manual_recieve_handshake,
            sign_out,
            // Remote
            use_remote,
            gen_drop_url,
            fetch_drop_object,
            check_online,
            // Library
            fetch_library,
            fetch_game,
            add_download_dir,
            delete_download_dir,
            fetch_download_dir_stats,
            fetch_game_status,
            fetch_game_version_options,
            update_game_configuration,
            #[cfg(target_os = "linux")]
            add_to_steam,
            // Downloads
            download_game,
            resume_download,
            move_download_in_queue,
            pause_downloads,
            resume_downloads,
            cancel_game,
            uninstall_game,
            // Processes
            launch_game,
            kill_game,
            toggle_autostart,
            get_autostart_enabled,
            open_process_logs,
            open_game_install_dir,
            #[cfg(target_os = "linux")]
            open_game_wine_prefix,
            #[cfg(target_os = "linux")]
            install_winetricks_verb,
            #[cfg(target_os = "linux")]
            list_winetricks_verbs,
            #[cfg(target_os = "linux")]
            run_winecfg,
            get_launch_options,
            get_process_handlers,
            #[cfg(target_os = "linux")]
            ::process::compat::fetch_proton_paths,
            #[cfg(target_os = "linux")]
            ::process::compat::add_proton_layer,
            #[cfg(target_os = "linux")]
            ::process::compat::remove_proton_layer,
            #[cfg(target_os = "linux")]
            ::process::compat::set_default
        ])
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimize"]),
        ))
        .setup(|app| {
            let handle = app.handle().clone();

            tauri::async_runtime::block_on(async move {
                let state = setup(handle.clone()).await;
                info!("initialized drop client");
                app.manage(Mutex::new(state));

                let global_app_handle = handle;
                {
                    let mut app_handle_lock = DROP_APP_HANDLE.lock().await;
                    app_handle_lock.replace(global_app_handle);
                };

                {
                    use tauri_plugin_deep_link::DeepLinkExt;
                    let _ = app.deep_link().register_all();
                    debug!("registered all pre-defined deep links");
                }

                let handle = app.handle().clone();

                let width = 1536.0;
                let height = 864.0;

                let main_window = WindowBuilder::new(&handle, "main")
                    .title("Drop Desktop App")
                    .min_inner_size(1000.0, 500.0)
                    .inner_size(width, height)
                    .decorations(false)
                    .shadow(false)
                    .visible(false)
                    .build()
                    .expect("failed to build main window");

                main_window
                    .add_child(
                        WebviewBuilder::new("frontend", WebviewUrl::App("main".into()))
                            .auto_resize(),
                        LogicalPosition::new(0., 0.),
                        LogicalSize::new(width, height),
                    )
                    .expect("failed to create frontend webview");

                // On Linux/Windows, `on_open_url` only fires when a *second*
                // instance is forwarded to this already-running one (via the
                // single-instance plugin) -- it is NOT emitted for the
                // initial launch itself (e.g. Steam launching Drop fresh
                // with `drop://launch/<id>` as a plain CLI argument, when
                // Drop wasn't already running). That initial URL has to be
                // read separately via `get_current()`.
                //
                // The window is built hidden and only shown here for a
                // normal launch -- if Steam cold-started Drop purely to run
                // a game via its shortcut, the user just wants the game, not
                // Drop's own window popping up too.
                let launched_to_run_a_game = app
                    .deep_link()
                    .get_current()
                    .ok()
                    .flatten()
                    .map(|urls| {
                        let mut is_game_launch = false;
                        for url in &urls {
                            is_game_launch |= url.host_str() == Some("launch");
                            handle_deep_link_url(url, &handle);
                        }
                        is_game_launch
                    })
                    .unwrap_or(false);

                if !launched_to_run_a_game
                    && let Err(e) = main_window.show()
                {
                    warn!("failed to show main window: {e}");
                }

                app.deep_link().on_open_url(move |event| {
                    debug!("handling drop:// url");
                    for url in event.urls() {
                        handle_deep_link_url(&url, &handle);
                    }
                });
                let open_menu_item = MenuItem::with_id(app, "open", "Open", true, None::<&str>)
                    .expect("Failed to generate open menu item");

                let sep = PredefinedMenuItem::separator(app)
                    .expect("Failed to generate menu separator item");

                let quit_menu_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
                    .expect("Failed to generate quit menu item");

                let menu = Menu::with_items(
                    app,
                    &[
                        &open_menu_item,
                        &sep,
                        /*
                        &MenuItem::with_id(app, "show_library", "Library", true, None::<&str>)?,
                        &MenuItem::with_id(app, "show_settings", "Settings", true, None::<&str>)?,
                        &PredefinedMenuItem::separator(app)?,
                         */
                        &quit_menu_item,
                    ],
                )
                .expect("Failed to generate menu");

                if env::var("NO_TRAY_ICON").is_ok_and(|value| value.to_lowercase() == "true") {
                    TRAY_DISABLED.store(true, Ordering::Relaxed);
                } else if !tray_icon_supported() {
                    warn!(
                        "appindicator library not available at runtime, disabling system tray icon"
                    );
                    TRAY_DISABLED.store(true, Ordering::Relaxed);
                }

                run_on_tray(|| {
                    let tray = TrayIconBuilder::new()
                        .icon(
                            app.default_window_icon()
                                .expect("Failed to get default window icon")
                                .clone(),
                        )
                        .menu(&menu)
                        .on_menu_event(|app, event| match event.id.as_ref() {
                            "open" => match app.get_window("main") {
                                Some(window) => {
                                    #[cfg(target_os = "linux")]
                                    {
                                        // Resume webview rendering after showing
                                        app_emit!(app, "window_visibility_change", ());
                                    }

                                    if let Err(e) = window.show() {
                                        warn!("failed to show main window: {e}");
                                    }
                                }
                                None => warn!(
                                    "tray 'open' clicked but no main window exists"
                                ),
                            },
                            "quit" => {
                                app.exit(0);
                            }

                            _ => {
                                warn!("menu event not handled: {:?}", event.id);
                            }
                        })
                        .build(app);

                    if let Err(e) = tray {
                        warn!("failed to set up system tray icon, disabling tray: {e}");
                        TRAY_DISABLED.store(true, Ordering::Relaxed);
                    }
                });

                {
                    let mut db_handle = borrow_db_mut_checked();
                    if let Some(original) = db_handle.prev_database.take() {
                        let canonicalised = match original.canonicalize() {
                            Ok(o) => o,
                            Err(_) => original,
                        };
                        warn!(
                            "Database corrupted. Original file at {}",
                            canonicalised.display()
                        );
                        app.dialog()
                            .message(format!(
                                "Database corrupted. A copy has been saved at: {}",
                                canonicalised.display()
                            ))
                            .title("Database corrupted")
                            .show(|_| {});
                    }
                }

                tokio::spawn(async move { scheduler_task().await });
            });

            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("object", move |_ctx, request, responder| {
            tauri::async_runtime::spawn(async move {
                fetch_object_wrapper(request, responder).await;
            });
        })
        .register_asynchronous_uri_scheme_protocol("server", |_ctx, request, responder| {
            tauri::async_runtime::spawn(async move {
                handle_server_proto_wrapper(request, responder).await;
            });
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let quit_on_close = borrow_db_checked().settings.quit_on_close;
                if !quit_on_close {
                    run_on_tray(|| {
                        // Pause webview rendering before hiding to prevent idle CPU usage
                        #[cfg(target_os = "linux")]
                        {
                            let _ = window.app_handle().emit("window_visibility_change", ());
                        }

                        window.hide().expect("Failed to hide window in tray");
                        api.prevent_close();
                    });
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|_app_handle, event| {
        if let RunEvent::ExitRequested { code, api, .. } = event {
            let quit_on_close = borrow_db_checked().settings.quit_on_close;
            if !quit_on_close {
                run_on_tray(|| {
                    if code.is_none() {
                        api.prevent_exit();
                    }
                });
            }
        }
    });
}

static TRAY_DISABLED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
fn tray_icon_supported() -> bool {
    [
        "libayatana-appindicator3.so.1",
        "libappindicator3.so.1",
        "libayatana-appindicator3.so",
        "libappindicator3.so",
    ]
    .iter()
    .any(|name| unsafe { libloading::Library::new(name) }.is_ok())
}

#[cfg(not(target_os = "linux"))]
fn tray_icon_supported() -> bool {
    true
}

fn run_on_tray<T: FnOnce()>(f: T) {
    if TRAY_DISABLED.load(Ordering::Relaxed) {
        return;
    }
    (f)();
}

fn handle_deep_link_url(url: &Url, handle: &AppHandle) {
    if let Some("handshake") = url.host_str() {
        tauri::async_runtime::spawn(recieve_handshake(
            handle.clone(),
            url.path().to_string(),
        ));
    } else if let Some("launch") = url.host_str() {
        // Used by the "Add to Steam" non-Steam shortcut:
        // Steam launches Drop with this as an argument (handed off to us
        // here as a deep link). If the game only has one configured launch
        // option (or needs first-time setup, which ignores the index
        // entirely -- see ProcessManager::launch_process), start it
        // directly; otherwise open a small picker window so the user gets
        // the same choice they'd have launching from Drop's own UI.
        let game_id = url.path().trim_start_matches('/').to_string();
        if game_id.is_empty() {
            warn!("drop://launch/ deep link missing a game id");
            return;
        }

        info!("launching game {game_id} via deep link");

        let needs_setup = {
            let db_lock = borrow_db_checked();
            !matches!(
                db_lock.applications.game_statuses.get(&game_id),
                Some(GameDownloadStatus::Installed {
                    install_type: InstalledGameType::Installed,
                    ..
                })
            )
        };

        let launch_options = if needs_setup {
            Vec::new()
        } else {
            ::process::process_manager::ProcessManager::get_launch_options(game_id.clone())
                .unwrap_or_default()
        };

        if launch_options.len() > 1 {
            open_launch_picker_window(handle, &game_id);
        } else if let Err(e) = ::process::PROCESS_MANAGER.lock().launch_process(game_id, 0) {
            warn!("Failed to launch game via deep link: {e}");
        }
    } else {
        warn!("unhandled drop:// url: {url}");
    }
}

fn open_launch_picker_window(handle: &AppHandle, game_id: &str) {
    if let Some(window) = handle.get_window("launch-picker") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let windowed = borrow_db_checked().settings.windowed_launch_picker;

    let width = 420.0;
    let height = 480.0;

    let mut builder = WindowBuilder::new(handle, "launch-picker")
        .title("Choose how to launch")
        .decorations(false)
        .shadow(false);

    builder = if windowed {
        builder
            .inner_size(width, height)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .center()
    } else {
        builder.fullscreen(true)
    };

    let window = match builder.build() {
        Ok(window) => window,
        Err(e) => {
            warn!("failed to build launch picker window: {e}");
            return;
        }
    };

    // Fullscreen windows don't resolve to `width`/`height` above, so read
    // back whatever size was actually applied (monitor resolution) for the
    // child webview's initial size -- `auto_resize` keeps it in sync with
    // any resizes after that.
    let (child_width, child_height) = if windowed {
        (width, height)
    } else {
        match window.inner_size() {
            Ok(size) => {
                let scale = window.scale_factor().unwrap_or(1.0);
                (size.width as f64 / scale, size.height as f64 / scale)
            }
            Err(_) => (width, height),
        }
    };

    let encoded_id = urlencoding::encode(game_id);
    let webview_url = WebviewUrl::App(format!("main/launch-picker?id={encoded_id}").into());

    if let Err(e) = window.add_child(
        WebviewBuilder::new("launch-picker", webview_url).auto_resize(),
        LogicalPosition::new(0., 0.),
        LogicalSize::new(child_width, child_height),
    ) {
        warn!("failed to create launch picker webview: {e}");
    }
}

// Guards against two handshake attempts running concurrently - e.g. the
// browser's automatic `drop://handshake/...` redirect and a manually pasted
// token both arriving for the same login (the manual-entry fallback is
// shown a couple seconds into every attempt regardless of whether the
// automatic redirect is about to land). Submitting the same client/token
// pair to the server twice at once races its finalization step and comes
// back as an opaque 500, which surfaced client-side as a generic
// "invalid response" authentication failure.
static HANDSHAKE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

struct HandshakeGuard;
impl Drop for HandshakeGuard {
    fn drop(&mut self) {
        HANDSHAKE_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

// TODO: Refactor
pub async fn recieve_handshake(app: AppHandle, path: String) {
    if HANDSHAKE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        warn!("ignoring handshake request - one is already in progress");
        return;
    }
    let _guard = HandshakeGuard;

    // Tell the app we're processing
    app_emit!(&app, "auth/processing", ());

    let handshake_result = recieve_handshake_logic(&app, path).await;
    if let Err(e) = handshake_result {
        warn!("error with authentication: {e}");
        app_emit!(&app, "auth/failed", e.to_string());
        return;
    }

    let app_state = app.state::<Mutex<AppState>>();

    debug!("calling auth::setup to fetch user");
    let (app_status, user) = auth::setup().await;
    debug!("auth::setup returned, status = {app_status:?}");

    let mut state_lock = app_state.lock();

    state_lock.status = app_status;
    state_lock.user = user;

    let _ = clear_cached_object("collections");
    let _ = clear_cached_object("library");

    drop(state_lock);

    debug!("emitting auth/finished");
    app_emit!(&app, "auth/finished", ());
    debug!("auth/finished emitted");
}

// TODO: Refactor
async fn recieve_handshake_logic(app: &AppHandle, path: String) -> Result<(), RemoteAccessError> {
    let path_chunks: Vec<&str> = path.split('/').collect();
    if path_chunks.len() != 3 {
        app_emit!(app, "auth/failed", ());
        return Err(RemoteAccessError::HandshakeFailed(
            "failed to parse token".to_string(),
        ));
    }

    let base_url = {
        let handle = borrow_db_checked();
        Url::parse(handle.base_url.as_str())?
    };

    let client_id = path_chunks
        .get(1)
        .expect("Failed to get client id from path chunks");
    let token = path_chunks
        .get(2)
        .expect("Failed to get token from path chunks");
    let body = HandshakeRequestBody::new((client_id).to_string(), (token).to_string());

    let endpoint = base_url.join("/api/v1/client/auth/handshake")?;
    let client = DROP_CLIENT_ASYNC.clone();
    let response = client
        .post(endpoint)
        .json(&body)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?;
    debug!("handshake responsded with {}", response.status().as_u16());
    if !response.status().is_success() {
        return Err(RemoteAccessError::InvalidResponse(response.json().await?));
    }
    let response_struct: HandshakeResponse = response.json().await?;

    {
        let mut handle = borrow_db_mut_checked();
        handle.auth = Some(response_struct.into());
    }

    let web_token = {
        let header = generate_authorization_header();
        debug!("requesting web token");
        let token = client
            .post(base_url.join("/api/v1/client/user/webtoken")?)
            .header("Authorization", header)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await?;
        debug!("web token request responded with {}", token.status().as_u16());

        token.text().await?
    };
    debug!("web token received, fetching user");
    let mut handle = borrow_db_mut_checked();
    handle.auth.as_mut().unwrap().web_token = Some(web_token);
    drop(handle);

    Ok(())
}
