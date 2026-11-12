import type { HistoryEntry } from "../api/types.gen";
import { type Card, isVideo } from "../state";
import { clipText, fmtClock, fmtSize, relativeDay, siteName } from "./format";

/** What a card shows. */
export type Phase = "loading" | "info-error" | "ready" | "waiting" | "starting" | "running" | "done" | "stopped";

export function phaseOf(c: Card): Phase {
  if (c.status === "downloading") return c.jobId ? "running" : c.waitUntil !== undefined ? "waiting" : "starting";
  if (c.status === "error") return "stopped";
  return c.status;
}

export const cardTitle = (c: Card): string => c.title || c.info?.title || "Untitled";

/** The uploader, else the site name. */
const who = (c: Card): string => c.info?.uploader || siteName(c.url);

/** "MP4 1080p", "MKV", "M4A". */
export function formatLabel(c: Card): string {
  if (!c.format) return "";
  const name = c.format.toUpperCase();
  if (!isVideo(c.format)) return name;
  const quality = c.info?.formats.find((f) => f.id === c.selectedFormat)?.label;
  return quality ? `${name} ${quality}` : name;
}

const join = (parts: (string | false | undefined | null)[]): string => parts.filter(Boolean).join(" · ");
const duration = (c: Card) => (c.info?.duration ? fmtClock(c.info.duration) : "");

/** "Blender Foundation · 10:34", "TikTok · 0:58 · MP4 720p · metadata removed". */
export function cardMeta(c: Card): string {
  const p = phaseOf(c);
  if (p === "ready" || p === "loading" || p === "info-error") return join([who(c), duration(c)]);
  return join([who(c), duration(c), formatLabel(c), c.clip && clipText(c.clip), c.strip && "metadata removed"]);
}

/** The shorter phone meta line: "MP4 1080p · clip 0:05 to 0:30", "Cancelled at 63% · MKV 1080p". */
export function phoneMeta(c: Card): string {
  switch (phaseOf(c)) {
    case "ready":
      return join([who(c), duration(c)]);
    case "waiting":
    case "starting":
      return join([who(c), formatLabel(c)]);
    case "running":
      return join([formatLabel(c), c.clip && clipText(c.clip)]);
    case "done":
      return join([formatLabel(c), c.size != null && fmtSize(c.size), c.strip && "no metadata"]);
    case "stopped":
      return join([c.jobState === "cancelled" && cancelledAt(c), formatLabel(c)]);
    default:
      return "";
  }
}

const cancelledAt = (c: Card): string =>
  c.progress != null && c.progress > 0 ? `Cancelled at ${Math.round(c.progress)}%` : "Cancelled";

/** Why a stopped card stopped, and what Resume does. */
export function stoppedNote(c: Card): string {
  const cancelled = c.progress != null && c.progress > 0 ? `Cancelled at ${Math.round(c.progress)}%.` : "Download cancelled.";
  const base = c.jobState === "cancelled" ? cancelled : (c.error ?? "Download failed");
  return c.resumable ? `${base.replace(/\.$/, "")}. Resume keeps the part already downloaded.` : base;
}

/** "Morning routine in Lisbon.mp4 · 18 MB". */
export const doneNote = (c: Card): string => join([c.filename, c.size != null && fmtSize(c.size)]);

/** "MP4 · 18 MB · today", "FLAC · 212 MB · 3 days ago · file expired". */
export function historyNote(e: HistoryEntry, now?: Date): string {
  return join([e.format.toUpperCase(), e.size != null && fmtSize(e.size), relativeDay(e.created_at, now), !e.file_available && "file expired"]);
}

/** "MP4 · 18 MB · today", "FLAC · 3 days ago · expired". */
export function phoneHistoryNote(e: HistoryEntry, now?: Date): string {
  return e.file_available
    ? join([e.format.toUpperCase(), e.size != null && fmtSize(e.size), relativeDay(e.created_at, now)])
    : join([e.format.toUpperCase(), relativeDay(e.created_at, now), "expired"]);
}

/** Seconds left before a rate-limited card retries. */
export const waitLeft = (c: Card, now: number = Date.now()): number =>
  Math.max(0, Math.ceil(((c.waitUntil ?? now) - now) / 1000));
