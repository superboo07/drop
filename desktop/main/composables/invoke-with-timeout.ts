import { invoke as tauriInvoke } from "@tauri-apps/api/core";

/**
 * Wraps `invoke` with a hard timeout and start/end logging. Some `invoke`
 * calls have been observed to hang indefinitely on certain machines
 * (network-backed commands stalling with no error), which otherwise
 * permanently freezes the UI since nothing downstream ever resolves or
 * rejects. This guarantees a rejection instead, and (combined with the
 * log-forward plugin mirroring console output into the backend's debug log)
 * makes a single RUST_LOG=debug capture show exactly which call, if any,
 * stalled and for how long.
 *
 * Pass `timeoutMs: Infinity` for calls that are expected to legitimately run
 * long (downloads, installs, file-picker dialogs waiting on the user) -- you
 * still get the start/finish/duration logging, just no forced cutoff.
 */
export async function invokeWithTimeout<T>(
  command: string,
  args?: Record<string, unknown>,
  timeoutMs = 20_000,
): Promise<T> {
  const start = performance.now();
  console.log(`invoke ${command} started`, args ?? {});
  try {
    const result = await (timeoutMs === Infinity
      ? tauriInvoke<T>(command, args)
      : Promise.race([
          tauriInvoke<T>(command, args),
          new Promise<never>((_, reject) =>
            setTimeout(
              () =>
                reject(new Error(`"${command}" timed out after ${timeoutMs}ms`)),
              timeoutMs,
            ),
          ),
        ]));
    console.log(
      `invoke ${command} finished in ${Math.round(performance.now() - start)}ms`,
    );
    return result;
  } catch (e) {
    console.error(
      `invoke ${command} failed after ${Math.round(performance.now() - start)}ms:`,
      e,
    );
    throw e;
  }
}
