// Shared between the client log upload route (counting problems) and the
// admin log viewer (highlighting lines), so both agree on what a line is.

export type ClientLogLevel = "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE";

export const clientLogLevels: ClientLogLevel[] = [
  "ERROR",
  "WARN",
  "INFO",
  "DEBUG",
  "TRACE",
];

// The client's own log: "{date} | {level} | {file}:{line} - {message}"
const dropLine = /^(\S+) \| (ERROR|WARN|INFO|DEBUG|TRACE) \| (\S+) - (.*)$/;
// Everything else (game output, stderr): "ERROR: ...", "WARNING ...",
// a tool's name first ("Proton: Error ...", "ProtonFixes[5860] WARN: ..."),
// GLib's "(drop-app:123): Gtk-WARNING **: ..." and Wine's "err:module:..."
const prefixedLine =
  /^(?:\([^)]*\): )?(?:[\w-]+(?:\[\d+\])?:\s+|[\w-]+\[\d+\]\s+)?(?:[\w-]+-)?(ERROR|FATAL|CRITICAL|WARNING|WARN|INFO|DEBUG)\b|^(err|warn|fixme):/i;
// Xlib's last words when the X server goes away under a game
const xlibFatal = /^XIO:\s+fatal/;

export function parseDropLogLine(line: string) {
  const match = line.match(dropLine);
  if (!match) return undefined;
  return {
    time: match[1],
    level: match[2] as ClientLogLevel,
    source: match[3],
    message: match[4],
  };
}

export function classifyLogLine(line: string): ClientLogLevel | undefined {
  const drop = parseDropLogLine(line);
  if (drop) return drop.level;

  if (xlibFatal.test(line)) return "ERROR";

  const match = line.match(prefixedLine);
  if (!match) return undefined;
  switch ((match[1] ?? match[2]).toUpperCase()) {
    case "ERROR":
    case "FATAL":
    case "CRITICAL":
    case "ERR":
      return "ERROR";
    case "WARNING":
    case "WARN":
      return "WARN";
    case "INFO":
      return "INFO";
    case "FIXME":
    case "DEBUG":
      return "DEBUG";
  }
  return undefined;
}

export function countLogProblems(content: string) {
  let errors = 0;
  let warnings = 0;
  for (const line of content.split("\n")) {
    const level = classifyLogLine(line);
    if (level === "ERROR") errors++;
    else if (level === "WARN") warnings++;
  }
  return { errors, warnings };
}

export type ClientLogTriggerSlug = "manual" | "crash" | "launch";

export const clientLogTriggerStyles: Record<
  ClientLogTriggerSlug,
  { class: string }
> = {
  launch: { class: "bg-red-400/10 text-red-400 ring-red-400/20" },
  crash: { class: "bg-orange-400/10 text-orange-300 ring-orange-400/20" },
  manual: { class: "bg-zinc-400/10 text-zinc-300 ring-zinc-400/20" },
};

export type ClientLogFile = {
  name: string;
  label: string;
  path: string;
  size: number;
  errors: number;
  warnings: number;
  truncated: boolean;
};

export type ClientLogSummary = {
  title: string;
  detail?: string;
  lastError?: string;
};

// [name, value, flagged] - flagged values differ from a normal desktop start
export type ClientLogEnvironment = Array<[string, string, boolean]>;
