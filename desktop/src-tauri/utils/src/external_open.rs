use std::process::Command;

/// Strips the environment variables that AppImage's `AppRun` (and its
/// gtk-hook) inject so our own bundled webkit2gtk/GTK stack loads correctly.
/// Those variables have no legitimate purpose for an *external* process
/// (a file manager, browser, or game/launcher) and can only cause harm:
/// `LD_LIBRARY_PATH` can shadow the external process's own native libs with
/// our bundled (different-version) ones, and `PYTHONHOME`/`PYTHONPATH`
/// point into our (Python-less) bundle, breaking any Python-based tool that
/// inherits them (e.g. `umu-run` failing with "Failed to import encodings
/// module"). Not gated on `cfg(target_os = "linux")` since on other
/// platforms these variables are simply never set, so this is a no-op.
pub fn sanitize_external_command(command: &mut Command) {
    const STRIP_VARS: &[&str] = &[
        "LD_LIBRARY_PATH",
        "PYTHONHOME",
        "PYTHONPATH",
        "PYTHONDONTWRITEBYTECODE",
        "GTK_DATA_PREFIX",
        "GTK_THEME",
        "GDK_BACKEND",
        "GSETTINGS_SCHEMA_DIR",
        "GTK_EXE_PREFIX",
        "GTK_PATH",
        "GTK_IM_MODULE_FILE",
        "GDK_PIXBUF_MODULE_FILE",
        "GIO_EXTRA_MODULES",
        "APPDIR",
        "APPIMAGE",
        "ARGV0",
    ];
    for var in STRIP_VARS {
        command.env_remove(var);
    }

    // AppRun doesn't replace PATH/XDG_DATA_DIRS outright, it prefixes them
    // with the bundle's own dirs (the original values are still present
    // later in the list) -- but that means a bundled binary can shadow a
    // same-named system one (this is exactly how the bundled, outdated
    // xdg-open ended up running instead of the system's). Strip just the
    // bundle's own segments rather than the whole variable.
    if let Ok(appdir) = std::env::var("APPDIR") {
        if let Ok(value) = std::env::var("XDG_DATA_DIRS") {
            let cleaned = value
                .split(':')
                .filter(|segment| !segment.starts_with(appdir.as_str()))
                .collect::<Vec<_>>()
                .join(":");
            command.env("XDG_DATA_DIRS", cleaned);
        }

        if let Ok(value) = std::env::var("PATH") {
            let mut cleaned = value
                .split(':')
                .filter(|segment| !segment.starts_with(appdir.as_str()))
                .collect::<Vec<_>>()
                .join(":");

            // Unlike usr/bin (which holds problematic bundled tools like
            // the outdated xdg-open), usr/libexec/drop-tools holds
            // umu-run/winetricks we deliberately vendor for systems with
            // no distro package manager to install them on (e.g. the
            // Steam Deck). Add it back so both a direct
            // `Command::new("winetricks")` and umu-run's own internal
            // PATH-based lookup of "winetricks" can find them.
            let vendor_dir = std::path::Path::new(&appdir).join("usr/libexec/drop-tools");
            if vendor_dir.is_dir() {
                cleaned = format!("{}:{cleaned}", vendor_dir.to_string_lossy());
            }

            command.env("PATH", cleaned);
        }
    }
}

/// Opens a file path or URL with the system's default handler, bypassing
/// both the AppImage's bundled (older, Plasma-6-incompatible) `xdg-open`
/// and the environment pollution described in [`sanitize_external_command`].
/// Falls back to `fallback` (e.g. the Tauri opener plugin, or the
/// `webbrowser` crate) if the system's `xdg-open` isn't available, or on
/// non-Linux platforms.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub fn open_externally<E>(
    target: &str,
    fallback: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    #[cfg(target_os = "linux")]
    {
        let mut command = Command::new("/usr/bin/xdg-open");
        command.arg(target);
        sanitize_external_command(&mut command);
        if command.spawn().is_ok() {
            return Ok(());
        }
    }

    fallback()
}
