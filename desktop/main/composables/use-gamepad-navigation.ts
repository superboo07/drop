// Polls the Gamepad API on an animation frame loop (it has no native
// "connected"/"button pressed" events on most platforms) and turns D-pad,
// left-stick, and A/B button presses into edge-triggered move/confirm/cancel
// callbacks - the caller owns what "move" means (e.g. clamping a selected
// index), this just debounces held buttons into single presses.
const AXIS_THRESHOLD = 0.5;
const DPAD_UP = 12;
const DPAD_DOWN = 13;
const BUTTON_CONFIRM = 0; // A / Cross
const BUTTON_CANCEL = 1; // B / Circle

export interface GamepadNavigationOptions {
  onMove: (delta: -1 | 1) => void;
  onConfirm: () => void;
  onCancel?: () => void;
}

export function useGamepadNavigation(options: GamepadNavigationOptions) {
  let frame: number | null = null;
  let wasUp = false;
  let wasDown = false;
  let wasConfirm = false;
  let wasCancel = false;

  function poll() {
    const pads = navigator.getGamepads?.() ?? [];
    for (const pad of pads) {
      if (!pad) continue;

      const stickY = pad.axes[1] ?? 0;
      const isUp = (pad.buttons[DPAD_UP]?.pressed ?? false) || stickY < -AXIS_THRESHOLD;
      const isDown =
        (pad.buttons[DPAD_DOWN]?.pressed ?? false) || stickY > AXIS_THRESHOLD;
      const isConfirm = pad.buttons[BUTTON_CONFIRM]?.pressed ?? false;
      const isCancel = pad.buttons[BUTTON_CANCEL]?.pressed ?? false;

      if (isUp && !wasUp) options.onMove(-1);
      if (isDown && !wasDown) options.onMove(1);
      if (isConfirm && !wasConfirm) options.onConfirm();
      if (isCancel && !wasCancel) options.onCancel?.();

      wasUp = isUp;
      wasDown = isDown;
      wasConfirm = isConfirm;
      wasCancel = isCancel;
    }

    frame = requestAnimationFrame(poll);
  }

  onMounted(() => {
    if (typeof navigator === "undefined" || !navigator.getGamepads) return;
    frame = requestAnimationFrame(poll);
  });

  onUnmounted(() => {
    if (frame !== null) cancelAnimationFrame(frame);
  });
}
