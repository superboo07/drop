import { getCurrentWindow } from "@tauri-apps/api/window";

// Root font-size scaling every rem-based Tailwind utility resolves against
// (see main.scss). Proportional to the viewport, not a flat px value, so it
// composes correctly under gamescope: gamescope's "resolution" setting
// doesn't change the physical display, it renders at that internal
// resolution and then uniformly stretches the composited output to fill the
// real screen (the same trick it uses for game upscaling). A flat px value
// doesn't compensate for that stretch at all - dropping gamescope from 4K to
// 720p would make the UI balloon, since the same fixed pixel count now
// covers a much bigger fraction of a smaller internal surface that then
// gets stretched back up to the same physical size. A value proportional to
// the viewport does compensate: it shrinks by the same factor gamescope's
// internal resolution shrank by, and gamescope's stretch-to-fit cancels
// that back out, leaving a constant final on-screen size regardless of
// which internal resolution was chosen.
//
// REFERENCE_VMIN anchors this to the main window's default (non-fullscreen)
// size (1536x864 -> min(w,h)/100 = 8.64) so it's an exact no-op there (an
// earlier version that scaled up even the default window by ~35% overflowed
// the custom titlebar).
//
// Only allowed to scale *down* below the reference point under gamescope
// specifically, not fullscreen in general:
//  - Fullscreen under gamescope: must scale down too, not just up, or a
//    lower internal resolution than the reference doesn't get compensated
//    and ends up too big once gamescope stretches it back out to the real
//    display.
//  - Fullscreen on a real display at a genuinely low resolution (no
//    gamescope): there's no compositor stretch to compensate for, so
//    shrinking here would make an already-small display's UI smaller still
//    - the opposite of what a couch/TV context wants.
//  - Windowed: a small window is just a small window (the user resizing it,
//    or the launch picker's compact dialog size) - same reasoning, no
//    stretch to compensate for.
const REFERENCE_VMIN = 8.64;
const ABSOLUTE_MIN_PX = 10;
const CEILING_MULTIPLIER = 3;

export function computeUiFontSizePx(uiScale: number, allowShrink: boolean): number {
  const base = 16 * uiScale;
  if (typeof window === "undefined") return base;
  const vmin = Math.min(window.innerWidth, window.innerHeight) / 100;
  const proportional = base * (vmin / REFERENCE_VMIN);
  const lowerBound = allowShrink ? ABSOLUTE_MIN_PX : base;
  return Math.min(Math.max(proportional, lowerBound), base * CEILING_MULTIPLIER);
}

export function applyUiFontSize(uiScale: number, allowShrink: boolean) {
  document.documentElement.style.fontSize = `${computeUiFontSizePx(uiScale, allowShrink)}px`;
}

// Keeps the scale correct if the viewport changes while the app is running
// (gamescope's internal resolution can be changed live, not just a normal
// window resize) - call once. allowShrink is only computed once at mount:
// nothing in this app toggles a window's fullscreen state or gamescope-ness
// mid-session, so there's no need to re-check it on every resize.
export function useLiveUiScale(uiScale: Ref<number>) {
  let allowShrink = false;

  function reapply() {
    applyUiFontSize(uiScale.value, allowShrink);
  }

  watch(uiScale, reapply);

  onMounted(async () => {
    const [isFullscreen, isGamescope] = await Promise.all([
      getCurrentWindow()
        .isFullscreen()
        .catch(() => false),
      invokeWithTimeout<boolean>("is_gamescope").catch(() => false),
    ]);
    allowShrink = isFullscreen && isGamescope;
    reapply();
    window.addEventListener("resize", reapply);
  });

  onUnmounted(() => {
    window.removeEventListener("resize", reapply);
  });
}
