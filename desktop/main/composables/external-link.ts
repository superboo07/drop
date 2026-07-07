// Plain `<a target="_blank">` links rely on the webview's own handling of
// external navigation, which silently does nothing in some environments
// (e.g. the Linux AppImage build). Route external links through the
// backend instead, which knows how to open them reliably.
export function openExternalLink(url: string) {
  invokeWithTimeout("open_fs", { path: url }).catch((error) => {
    console.error(`Failed to open external link "${url}":`, error);
  });
}
