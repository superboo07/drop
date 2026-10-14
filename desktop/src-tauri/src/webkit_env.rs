//! WebKitGTK settings that only take effect if made before WebKit starts.

use std::{
    env,
    sync::atomic::{AtomicBool, Ordering},
};

use log::info;
use utils::render_node::has_drm_render_node;

const DISABLE_DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

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
