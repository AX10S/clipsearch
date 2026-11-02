// ─────────────────────────────────────────────────────────────────────────────
// Small utility helpers used across the UI.
// ─────────────────────────────────────────────────────────────────────────────

/** Format a Unix-millis timestamp as a short relative time ("2m", "5h", "3d"). */
export function formatRelativeTime(ms: number, now = Date.now()): string {
  const delta = Math.max(0, now - ms);
  const sec = Math.floor(delta / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}d`;
  const mon = Math.floor(day / 30);
  if (mon < 12) return `${mon}mo`;
  const yr = Math.floor(day / 365);
  return `${yr}y`;
}

/** Truncate a string to `max` chars, appending an ellipsis if truncated. */
export function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max - 1) + "…";
}

/** Format a byte count as a human-readable string. */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

/** Debounce any async function by `ms` milliseconds. */
export function debounce<A extends unknown[], R>(
  fn: (...args: A) => Promise<R>,
  ms: number
): (...args: A) => Promise<R> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending: Array<(v: R | PromiseLike<R>) => void> = [];
  let pendingReject: Array<(e: unknown) => void> = [];
  let latestArgs: A | null = null;

  return (...args: A): Promise<R> => {
    latestArgs = args;
    if (timer) clearTimeout(timer);
    return new Promise<R>((resolve, reject) => {
      pending.push(resolve);
      pendingReject.push(reject);
      timer = setTimeout(async () => {
        timer = null;
        const args = latestArgs!;
        const resolvers = pending;
        const rejecters = pendingReject;
        pending = [];
        pendingReject = [];
        try {
          const result = await fn(...args);
          for (const r of resolvers) r(result);
        } catch (e) {
          for (const r of rejecters) r(e);
        }
      }, ms);
    });
  };
}

/** Detect the OS-level platform modifier name for keyboard hints. */
export function platformModifierLabel(): string {
  const ua = navigator.userAgent;
  if (ua.includes("Mac")) return "⌘";
  if (ua.includes("Win")) return "Ctrl";
  return "Ctrl";
}

/** Returns true if the current platform is macOS. */
export function isMac(): boolean {
  return navigator.userAgent.includes("Mac");
}
