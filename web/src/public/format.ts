import type { Clip, JobState } from "../api/types.gen";

const KIB = 1024;
const MIB = KIB * KIB;
const GIB = MIB * KIB;

/** "1:20", "1:02:03". */
export function fmtClock(secs: number): string {
  const t = Math.max(0, Math.round(secs));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = String(t % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** "840 KB", "3.1 MB", "18 MB", "1.2 GB". */
export function fmtSize(bytes: number): string {
  if (bytes >= GIB) return `${(bytes / GIB).toFixed(1)} GB`;
  if (bytes >= 10 * MIB) return `${Math.round(bytes / MIB)} MB`;
  if (bytes >= MIB) return `${(bytes / MIB).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / KIB))} KB`;
}

export type ProgressInfo = {
  progress?: number | null;
  speed?: number | null;
  eta?: number | null;
  state?: JobState;
};

/** "42% · 3.1 MB/s · 0:18 left"; missing parts are left out. */
export function formatProgress(p: ProgressInfo): string {
  if (p.state === "queued") return "Queued";
  if (p.state === "processing") return "Processing";
  const parts: string[] = [];
  if (p.progress != null) parts.push(`${Math.round(p.progress)}%`);
  if (p.speed != null) parts.push(p.speed >= MIB ? `${(p.speed / MIB).toFixed(1)} MB/s` : `${Math.round(p.speed / KIB)} KB/s`);
  if (p.eta != null) parts.push(`${fmtClock(p.eta)} left`);
  return parts.join(" · ") || "Starting...";
}

/** Width of the progress bar fill, 0 to 100. */
export const progressWidth = (p: ProgressInfo): number =>
  p.state === "processing" ? 100 : Math.min(100, Math.max(0, p.progress ?? 0));

/** "80", "1:20" or "1:02:03" to seconds; "" is undefined, anything else NaN. */
export function parseClock(text: string): number | undefined {
  const t = text.trim();
  if (t === "") return undefined;
  if (!/^\d+(:\d{1,2}){0,2}$/.test(t)) return Number.NaN;
  const parts = t.split(":").map(Number);
  if (parts.slice(1).some((n) => n >= 60)) return Number.NaN;
  return parts.reduce((acc, n) => acc * 60 + n, 0);
}

export type ClipInput = { on: boolean; from: string; to: string };

/** The clip of the next downloads, or an error text when the times do not make sense. */
export function clipFromInput(input: ClipInput): { clip?: Clip; error?: string } {
  if (!input.on) return {};
  const start = parseClock(input.from) ?? 0;
  const end = parseClock(input.to);
  if (Number.isNaN(start) || (end !== undefined && Number.isNaN(end))) return { error: "Clip times look like 1:20 or 80" };
  if (end !== undefined && end <= start) return { error: "Clip end must come after its start" };
  return { clip: end === undefined ? { start } : { start, end } };
}

/** "clip 0:05 to 0:30", "clip from 1:20". */
export function clipText(clip: Clip): string {
  return clip.end == null ? `clip from ${fmtClock(clip.start)}` : `clip ${fmtClock(clip.start)} to ${fmtClock(clip.end)}`;
}

/** "6 h", "45 min", "2 days". */
export function fmtTtl(secs: number): string {
  if (secs >= 2 * 86400 && secs % 86400 === 0) return `${secs / 86400} days`;
  if (secs >= 3600) return `${Math.round(secs / 3600)} h`;
  return `${Math.max(1, Math.round(secs / 60))} min`;
}

/** "today", "yesterday", "3 days ago" (calendar days, local time). */
export function relativeDay(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  const day = (d: Date) => Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 86_400_000;
  const days = Math.max(0, Math.round(day(now) - day(then)));
  if (days === 0) return "today";
  if (days === 1) return "yesterday";
  return `${days} days ago`;
}

const SITES: [RegExp, string][] = [
  [/(^|\.)(youtube\.com|youtu\.be)$/, "YouTube"],
  [/(^|\.)tiktok\.com$/, "TikTok"],
  [/(^|\.)instagram\.com$/, "Instagram"],
  [/(^|\.)(twitter\.com|x\.com)$/, "X"],
  [/(^|\.)reddit\.com$/, "Reddit"],
  [/(^|\.)(facebook\.com|fb\.watch)$/, "Facebook"],
  [/(^|\.)vimeo\.com$/, "Vimeo"],
  [/(^|\.)twitch\.tv$/, "Twitch"],
  [/(^|\.)(bilibili\.com|b23\.tv)$/, "Bilibili"],
  [/(^|\.)soundcloud\.com$/, "SoundCloud"],
];

/** "YouTube" for a known site, else the host without "www.". */
export function siteName(url: string): string {
  let host: string;
  try {
    host = new URL(url).hostname.toLowerCase();
  } catch {
    return "";
  }
  return SITES.find(([re]) => re.test(host))?.[1] ?? host.replace(/^www\./, "");
}
