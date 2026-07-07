import { invoke } from "@tauri-apps/api/core";

// Mirrors console.* into the backend's debug log (see log_frontend Rust
// command), so a single RUST_LOG=debug capture shows frontend and backend
// activity interleaved in one place instead of needing the webview
// inspector open separately to see what the frontend was doing.
export default defineNuxtPlugin({
  name: "log-forward",
  enforce: "pre",
  setup() {
    const original = {
      log: console.log.bind(console),
      warn: console.warn.bind(console),
      error: console.error.bind(console),
    };

    const stringify = (value: unknown): string => {
      if (typeof value === "string") return value;
      if (value instanceof Error) return `${value.message}\n${value.stack}`;
      try {
        return JSON.stringify(value);
      } catch {
        return String(value);
      }
    };

    const forward = (level: string, args: unknown[]) => {
      const message = args.map(stringify).join(" ");
      invoke("log_frontend", { level, message }).catch(() => {});
    };

    console.log = (...args: unknown[]) => {
      original.log(...args);
      forward("debug", args);
    };
    console.warn = (...args: unknown[]) => {
      original.warn(...args);
      forward("warn", args);
    };
    console.error = (...args: unknown[]) => {
      original.error(...args);
      forward("error", args);
    };
  },
});
