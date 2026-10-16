import type { LogLevel, LogLine } from "@/ipc/types";

/** The levels, most severe first: the order the level filter lists them in. */
export const LOG_LEVELS: readonly LogLevel[] = ["error", "warn", "info", "debug", "trace"];

/** As many lines as the backend keeps; the window holds no more than it. */
export const LOG_WINDOW_LINES = 10_000;

/** The lines at `minimum` or more severe whose text holds `query`, case aside. */
export function visibleLogLines(lines: readonly LogLine[], minimum: LogLevel, query: string): LogLine[] {
  const rank = LOG_LEVELS.indexOf(minimum);
  const needle = query.trim().toLowerCase();
  return lines.filter(line => LOG_LEVELS.indexOf(line.level) <= rank
    && (needle === "" || `${line.target} ${line.message}`.toLowerCase().includes(needle)));
}

/** A line as the log file writes it: UTC time, level, module, message. */
export function formatLogLine(line: LogLine): string {
  return `${new Date(line.timeMs).toISOString()} ${line.level.toUpperCase().padStart(5)} ${line.target}: ${line.message}`;
}

/** New lines added after the held ones, keeping only the newest `LOG_WINDOW_LINES`. */
export function appendLogLines(held: readonly LogLine[], incoming: readonly LogLine[]): LogLine[] {
  const last = held.at(-1)?.seq ?? 0;
  const fresh = incoming.filter(line => line.seq > last);
  if (fresh.length === 0) return held as LogLine[];
  const joined = [...held, ...fresh];
  return joined.length > LOG_WINDOW_LINES ? joined.slice(joined.length - LOG_WINDOW_LINES) : joined;
}
