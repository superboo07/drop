//! WebKitGTK settings that only take effect if made before WebKit starts.

use std::{
    env, fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    sync::atomic::{AtomicBool, Ordering},
};

use log::info;

const DISABLE_DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

/// The char major of every DRM device (`DRM_MAJOR` in the kernel).
const DRM_MAJOR: u64 = 226;

static DMABUF_DISABLED: AtomicBool = AtomicBool::new(false);

/// WebKitGTK's DMA-BUF renderer has the web process draw into buffers
/// allocated with GBM on a DRM render node and hands them to the window. Where
/// `/dev/dri` has no real render node, that fails without a word and the window
/// stays grey: DroidDeck, for one, has a `renderD0` that is its own bridge to
/// the phone's GPU (a dynamically assigned char major, not DRM's), which Mesa
/// renders through but GBM cannot allocate on. There, WebKit is switched to its
/// shared-memory path, which works anywhere. Systems with a render node keep
/// the faster DMA-BUF path, and a value the user set is always left alone.
///
/// Must run before any other thread exists: it sets an environment variable.
pub fn apply() {
    if env::var_os(DISABLE_DMABUF).is_some() || has_drm_render_node() {
        return;
    }
    // SAFETY: called first thing in `run()`, while this is the only thread.
    unsafe { env::set_var(DISABLE_DMABUF, "1") };
    DMABUF_DISABLED.store(true, Ordering::Relaxed);
}

/// Logs what [`apply`] decided, once logging is set up.
pub fn log_decision() {
    if DMABUF_DISABLED.load(Ordering::Relaxed) {
        info!(
            "no DRM render node in /dev/dri, so WebKit's DMA-BUF renderer is off ({DISABLE_DMABUF}=1)"
        );
    }
}

fn has_drm_render_node() -> bool {
    let Ok(entries) = fs::read_dir("/dev/dri") else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("renderD"))
            && fs::metadata(entry.path()).is_ok_and(|metadata| {
                metadata.file_type().is_char_device() && major(metadata.rdev()) == DRM_MAJOR
            })
    })
}

/// glibc's `major()`: the device number's major half, from either encoding.
fn major(dev: u64) -> u64 {
    ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)
}
