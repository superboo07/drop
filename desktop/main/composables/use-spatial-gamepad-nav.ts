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

function findNextCandidate(
  current: HTMLElement,
  direction: Direction,
  candidates: HTMLElement[],
): HTMLElement | null {
  const from = current.getBoundingClientRect();
  const fromCenter = { x: from.left + from.width / 2, y: from.top + from.height / 2 };

  let best: HTMLElement | null = null;
  let bestScore = Infinity;

  for (const el of candidates) {
    if (el === current) continue;
    const rect = el.getBoundingClientRect();
    const center = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    const dx = center.x - fromCenter.x;
    const dy = center.y - fromCenter.y;

    let primary: number;
    let cross: number;
    switch (direction) {
      case "up":
        if (dy >= -1) continue;
        primary = -dy;
        cross = Math.abs(dx);
        break;
      case "down":
        if (dy <= 1) continue;
        primary = dy;
        cross = Math.abs(dx);
        break;
      case "left":
        if (dx >= -1) continue;
        primary = -dx;
        cross = Math.abs(dy);
        break;
      case "right":
        if (dx <= 1) continue;
        primary = dx;
        cross = Math.abs(dy);
        break;
    }

    // Weight the cross-axis offset heavier than the primary-axis distance so
    // moving "down" prefers the element directly below over one further away
    // but more precisely aligned diagonally.
    const score = primary + cross * 2;
    if (score < bestScore) {
      bestScore = score;
      best = el;
    }
  }

  return best;
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
  const active = document.activeElement as HTMLElement | null;
  if (!active || active === document.body) {
    focusInitial();
    return;
  }
  active.click();
}

function cancel() {
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

    for (const pad of pads) {
      if (!pad) continue;

      const stickX = pad.axes[0] ?? 0;
      const stickY = pad.axes[1] ?? 0;

      handleDirection(
        "up",
        (pad.buttons[DPAD_UP]?.pressed ?? false) || stickY < -AXIS_THRESHOLD,
        now,
      );
      handleDirection(
        "down",
        (pad.buttons[DPAD_DOWN]?.pressed ?? false) || stickY > AXIS_THRESHOLD,
        now,
      );
      handleDirection(
        "left",
        (pad.buttons[DPAD_LEFT]?.pressed ?? false) || stickX < -AXIS_THRESHOLD,
        now,
      );
      handleDirection(
        "right",
        (pad.buttons[DPAD_RIGHT]?.pressed ?? false) || stickX > AXIS_THRESHOLD,
        now,
      );

      const isConfirm = pad.buttons[BUTTON_CONFIRM]?.pressed ?? false;
      const isCancel = pad.buttons[BUTTON_CANCEL]?.pressed ?? false;
      if (isConfirm && !wasConfirm) confirm();
      if (isCancel && !wasCancel) cancel();
      wasConfirm = isConfirm;
      wasCancel = isCancel;
    }

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
