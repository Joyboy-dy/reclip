import type { JobState } from "../api/types.gen";

export type ProgressInfo = {
  progress: number | null | undefined;
  speed: number | null | undefined;
  eta: number | null | undefined;
  state?: JobState;
};

const MIB = 1024 * 1024;

function fmtEta(s: number): string {
  const t = Math.max(0, Math.round(s));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const ss = String(t % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** "42 % · 3.1 MB/s · 0:12"; null parts are omitted. */
export function formatProgress(p: ProgressInfo): string {
  if (p.state === "queued") return "Queued";
  if (p.state === "processing") return "Processing";
  const parts: string[] = [];
  if (p.progress != null) parts.push(`${Math.round(p.progress)} %`);
  if (p.speed != null) {
    parts.push(p.speed >= MIB ? `${(p.speed / MIB).toFixed(1)} MB/s` : `${Math.round(p.speed / 1024)} KB/s`);
  }
  if (p.eta != null) parts.push(fmtEta(p.eta));
  return parts.join(" · ") || "Starting...";
}

const width = (p: ProgressInfo): number =>
  p.state === "processing" ? 100 : Math.min(100, Math.max(0, p.progress ?? 0));

const cls = (p: ProgressInfo): string =>
  "pbar" + (p.state === "queued" ? " queued" : p.state === "processing" ? " processing" : "");

/** Thin accent bar plus a DM Mono status line. The fill width is set via CSSOM by `updateProgressBar` once the markup is in the DOM (the CSP forbids inline styles). */
export function progressBarHtml(p: ProgressInfo): string {
  return `<div class="${cls(p)}"><div class="pbar-fill"></div></div><div class="pbar-text">${formatProgress(p)}</div>`;
}

/** Update an already rendered bar in place (no flicker of the thumbnail). */
export function updateProgressBar(root: HTMLElement, p: ProgressInfo): boolean {
  const bar = root.querySelector<HTMLElement>(".pbar");
  const fill = root.querySelector<HTMLElement>(".pbar-fill");
  const text = root.querySelector<HTMLElement>(".pbar-text");
  if (!bar || !fill || !text) return false;
  bar.className = cls(p);
  fill.style.width = `${width(p)}%`;
  text.textContent = formatProgress(p);
  return true;
}
