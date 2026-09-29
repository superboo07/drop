// App-wide gamepad navigation: polls the Gamepad API (no native "connected"/
// "button pressed" events on most platforms) and moves DOM focus between
// focusable elements using spatial nearest-neighbour matching, rather than a
// hand-maintained index per page. A-confirms by clicking whatever's focused,
// B-cancels by dispatching Escape (closes HeadlessUI dialogs, which already
// listen for it) or falling back to browser back navigation.
//
// Mounted once from app.vue, which also loads for the launch-picker window
// (a separate Tauri webview that loads the same built Nuxt app), so this
// covers that window too -- no per-page wiring needed.

import { getCurrentWindow } from "@tauri-apps/api/window";

const AXIS_THRESHOLD = 0.5;
const AXIS_RELEASE = 0.3;
const REPEAT_DELAY_MS = 400;
const REPEAT_RATE_MS = 120;

const DPAD_UP = 12;
const DPAD_DOWN = 13;
const DPAD_LEFT = 14;
const DPAD_RIGHT = 15;
const BUTTON_CONFIRM = 0; // A / Cross
const BUTTON_CANCEL = 1; // B / Circle

type Direction = "up" | "down" | "left" | "right";

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function isVisible(el: HTMLElement) {
  if (el.offsetParent === null && el.style.position !== "fixed") return false;
  const rect = el.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0;
}

function getNavRoot(): ParentNode {
  // Scope to the topmost open dialog so navigation doesn't reach through
  // the backdrop into the page behind it.
  const dialogs = document.querySelectorAll('[role="dialog"]');
  if (dialogs.length > 0) return dialogs[dialogs.length - 1];
  return document;
}

function getFocusableCandidates(): HTMLElement[] {
  const root = getNavRoot();
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    isVisible,
  );
}

// A rect's edges along the direction of travel ("start" is the near edge,
// "end" the far one, both increasing in that direction) and across it, so
// one scoring routine serves all four directions.
function directionalEdges(rect: DOMRect, direction: Direction) {
  switch (direction) {
    case "down":
      return { start: rect.top, end: rect.bottom, crossStart: rect.left, crossEnd: rect.right };
    case "up":
      return { start: -rect.bottom, end: -rect.top, crossStart: rect.left, crossEnd: rect.right };
    case "right":
      return { start: rect.left, end: rect.right, crossStart: rect.top, crossEnd: rect.bottom };
    case "left":
      return { start: -rect.right, end: -rect.left, crossStart: rect.top, crossEnd: rect.bottom };
  }
}

// Edge-based rather than center-based: comparing centers orders a tall
// element beside a short one by its middle instead of where it visually
// starts, so moving down would focus the short one first, then "jump back"
// to the tall one, then carry on past -- the selector skipping something
// and returning to it.
function findNextCandidate(
  current: HTMLElement,
  direction: Direction,
  candidates: HTMLElement[],
): HTMLElement | null {
  const from = directionalEdges(current.getBoundingClientRect(), direction);
  const fromCross = (from.crossStart + from.crossEnd) / 2;

  let best: HTMLElement | null = null;
  let bestScore = Infinity;
  let bestLoose: HTMLElement | null = null;
  let bestLooseScore = Infinity;

  for (const el of candidates) {
    if (el === current) continue;
    const to = directionalEdges(el.getBoundingClientRect(), direction);
    const centerCross = Math.abs((to.crossStart + to.crossEnd) / 2 - fromCross);
    const crossGap = Math.max(0, to.crossStart - from.crossEnd, from.crossStart - to.crossEnd);

    // Strictly ahead: both edges move forward. Excludes elements beside (or
    // nested inside) the current one -- those belong to the other axis.
    if (to.start > from.start + 1 && to.end > from.end + 1) {
      // Distance along the direction dominates, being off to the side costs
      // double, and among equally near, overlapping candidates the one
      // closest to straight ahead wins.
      const primaryGap = Math.max(0, to.start - from.end);
      const score = primaryGap + crossGap * 2 + centerCross * 0.1;
      if (score < bestScore) {
        bestScore = score;
        best = el;
      }
      continue;
    }

    // Fallback for overlapping layouts where nothing is strictly ahead: a
    // center further along still counts, so nothing becomes unreachable.
    const primary = (to.start + to.end) / 2 - (from.start + from.end) / 2;
    if (primary <= 1) continue;
    const score = primary + centerCross * 2;
    if (score < bestLooseScore) {
      bestLooseScore = score;
      bestLoose = el;
    }
  }

  return best ?? bestLoose;
}

// An open HeadlessUI Menu or Listbox keeps focus on its container
// (role="menu"/"listbox") and highlights items itself in response to arrow
// keys -- the items are tabindex="-1", so spatial navigation can't see them
// and would instead move focus out, which closes the popup. While one has
// focus, the pad is translated into the keys it already understands.
function focusedPopup(): HTMLElement | null {
  const active = document.activeElement as HTMLElement | null;
  return active?.closest<HTMLElement>('[role="menu"], [role="listbox"]') ?? null;
}

function sendKey(target: Element, key: string) {
  target.dispatchEvent(
    new KeyboardEvent("keydown", { key, code: key, bubbles: true, cancelable: true }),
  );
}

function markGamepadActive() {
  document.body.classList.add("gamepad-nav");
}

function focusInitial() {
  const candidates = getFocusableCandidates();
  candidates[0]?.focus();
}

function move(direction: Direction) {
  markGamepadActive();
  const popup = focusedPopup();
  if (popup) {
    // Both widgets list their items vertically; left/right have no meaning.
    if (direction === "up") sendKey(popup, "ArrowUp");
    if (direction === "down") sendKey(popup, "ArrowDown");
    return;
  }

  const active = document.activeElement;
  const candidates = getFocusableCandidates();

  if (!active || active === document.body || !candidates.includes(active as HTMLElement)) {
    candidates[0]?.focus();
    return;
  }

  const next = findNextCandidate(active as HTMLElement, direction, candidates);
  next?.focus();
}

function confirm() {
  markGamepadActive();
  const popup = focusedPopup();
  if (popup) {
    // Picks the highlighted item, as a keyboard user would.
    sendKey(popup, "Enter");
    return;
  }

  const active = document.activeElement as HTMLElement | null;
  if (!active || active === document.body) {
    focusInitial();
    return;
  }
  active.click();
}

// router.back() is async (it lands on popstate), so another B press before
// then would queue a second history step from the same page and overshoot.
let backPending = false;

function cancel() {
  const popup = focusedPopup();
  if (popup) {
    // Closes just the popup (focus returns to its button), rather than the
    // dialog or page it's on.
    sendKey(popup, "Escape");
    return;
  }

  const root = getNavRoot();
  if (root !== document) {
    // A dialog is open: ask it to close the way keyboard users already do.
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true }),
    );
    return;
  }

  const router = useRouter();
  if (router.currentRoute.value.path === "/launch-picker") {
    // A standalone popup window, not part of the main app's navigation
    // history -- B closes it, matching what Escape already does there.
    getCurrentWindow().close();
    return;
  }

  if (backPending) return;

  // Vue Router records the previous in-app location in history.state.back:
  // null on the first entry, and "/" is the blank bootstrap page
  // (pages/index.vue) that would leave the user on a white screen.
  const back = (window.history.state as { back?: string | null } | null)?.back;
  if (!back || router.resolve(back).path === "/") return;

  backPending = true;
  const done = () => {
    backPending = false;
    window.removeEventListener("popstate", done);
  };
  window.addEventListener("popstate", done);
  setTimeout(done, 1000);
  router.back();
}

export function useSpatialGamepadNavigation() {
  let frame: number | null = null;

  const held: Record<Direction, boolean> = {
    up: false,
    down: false,
    left: false,
    right: false,
  };
  const heldSince: Record<Direction, number> = { up: 0, down: 0, left: 0, right: 0 };
  let lastRepeat: Record<Direction, number> = { up: 0, down: 0, left: 0, right: 0 };
  let wasConfirm = false;
  let wasCancel = false;

  function handleDirection(direction: Direction, pressed: boolean, now: number) {
    if (!pressed) {
      held[direction] = false;
      return;
    }

    if (!held[direction]) {
      held[direction] = true;
      heldSince[direction] = now;
      lastRepeat[direction] = now;
      move(direction);
      return;
    }

    const heldFor = now - heldSince[direction];
    if (heldFor < REPEAT_DELAY_MS) return;
    if (now - lastRepeat[direction] < REPEAT_RATE_MS) return;
    lastRepeat[direction] = now;
    move(direction);
  }

  function poll() {
    const pads = navigator.getGamepads?.() ?? [];
    const now = performance.now();

    // Merge every connected pad into one state before edge-detecting. The
    // same physical controller often shows up twice (e.g. Steam Input's
    // virtual pad alongside the real one); handled per pad, the idle copy
    // would reset "held" every frame and the other would re-trigger a fresh
    // press each frame, skipping several elements (or going back several
    // pages) on one press.
    const pressed: Record<Direction, boolean> = {
      up: false,
      down: false,
      left: false,
      right: false,
    };
    let isConfirm = false;
    let isCancel = false;

    for (const pad of pads) {
      if (!pad) continue;

      // A stick direction engages past AXIS_THRESHOLD but only releases
      // below AXIS_RELEASE, so a stick hovering near the threshold doesn't
      // flicker into repeated presses.
      const stickX = pad.axes[0] ?? 0;
      const stickY = pad.axes[1] ?? 0;
      const stick = (value: number, direction: Direction) =>
        value > (held[direction] ? AXIS_RELEASE : AXIS_THRESHOLD);

      pressed.up ||= (pad.buttons[DPAD_UP]?.pressed ?? false) || stick(-stickY, "up");
      pressed.down ||= (pad.buttons[DPAD_DOWN]?.pressed ?? false) || stick(stickY, "down");
      pressed.left ||= (pad.buttons[DPAD_LEFT]?.pressed ?? false) || stick(-stickX, "left");
      pressed.right ||= (pad.buttons[DPAD_RIGHT]?.pressed ?? false) || stick(stickX, "right");

      isConfirm ||= pad.buttons[BUTTON_CONFIRM]?.pressed ?? false;
      isCancel ||= pad.buttons[BUTTON_CANCEL]?.pressed ?? false;
    }

    for (const direction of ["up", "down", "left", "right"] as const) {
      handleDirection(direction, pressed[direction], now);
    }

    if (isConfirm && !wasConfirm) confirm();
    if (isCancel && !wasCancel) cancel();
    wasConfirm = isConfirm;
    wasCancel = isCancel;

    frame = requestAnimationFrame(poll);
  }

  function clearGamepadActive() {
    document.body.classList.remove("gamepad-nav");
  }

  onMounted(() => {
    document.addEventListener("mousedown", clearGamepadActive);
    document.addEventListener("mousemove", clearGamepadActive);

    if (typeof navigator === "undefined" || !navigator.getGamepads) return;
    frame = requestAnimationFrame(poll);
  });

  onUnmounted(() => {
    document.removeEventListener("mousedown", clearGamepadActive);
    document.removeEventListener("mousemove", clearGamepadActive);
    if (frame !== null) cancelAnimationFrame(frame);
  });
}
