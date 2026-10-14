//! Whether the GPU can share buffers the usual way.

/// The char major of every DRM device (`DRM_MAJOR` in the kernel).
#[cfg(target_os = "linux")]
const DRM_MAJOR: u64 = 226;

/// Whether `/dev/dri` has a real DRM render node. Where it doesn't, anything
/// that shares GPU buffers as DMA-BUFs fails: DroidDeck, for one, has a
/// `renderD0` that is its own bridge to the phone's GPU (a dynamically
/// assigned char major, not DRM's), which Mesa renders through but which
/// can't export or allocate DMA-BUFs - its DRM ioctls fail with ENOTTY. That
/// greyed out WebKit's window (`webkit_env`) and closed games' windows as
/// soon as they presented a frame to X (`process_manager`).
#[cfg(target_os = "linux")]
pub fn has_drm_render_node() -> bool {
    use std::{
        fs,
        os::unix::fs::{FileTypeExt, MetadataExt},
    };

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
#[cfg(target_os = "linux")]
fn major(dev: u64) -> u64 {
    ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)
}
