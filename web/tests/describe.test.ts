import { expect, test } from "bun:test";
import type { HistoryEntry } from "../src/api/types.gen";
import { cardMeta, doneNote, formatLabel, historyNote, phaseOf, phoneHistoryNote, phoneMeta, stoppedNote, waitLeft } from "../src/public/describe";
import type { Card } from "../src/state";

const MB = 1024 * 1024;
const info = {
  title: "Big Buck Bunny",
  thumbnail: null,
  duration: 634,
  uploader: "Blender Foundation",
  formats: [
    { id: "137", label: "1080p", height: 1080, codec: null },
    { id: "313", label: "2160p VP9", height: 2160, codec: "VP9" },
  ],
};
const card = (over: Partial<Card>): Card => ({ key: "k", url: "https://www.youtube.com/watch?v=a", status: "ready", ...over });

test("phaseOf tells the card states apart", () => {
  expect(phaseOf(card({ status: "loading" }))).toBe("loading");
  expect(phaseOf(card({ status: "downloading" }))).toBe("starting");
  expect(phaseOf(card({ status: "downloading", waitUntil: 1 }))).toBe("waiting");
  expect(phaseOf(card({ status: "downloading", jobId: "j" }))).toBe("running");
  expect(phaseOf(card({ status: "error" }))).toBe("stopped");
  expect(phaseOf(card({ status: "done" }))).toBe("done");
});

test("desktop meta lines follow the mockup", () => {
  expect(cardMeta(card({ info }))).toBe("Blender Foundation · 10:34");
  const done = card({ info, status: "done", format: "mp4", selectedFormat: "137", strip: true });
  expect(cardMeta(done)).toBe("Blender Foundation · 10:34 · MP4 1080p · metadata removed");
  const running = card({ status: "downloading", jobId: "j", format: "mkv", clip: { start: 5, end: 30 } });
  expect(cardMeta(running)).toBe("YouTube · MKV · clip 0:05 to 0:30");
  expect(formatLabel(card({ info, format: "mov", selectedFormat: "313" }))).toBe("MOV 2160p VP9");
  expect(formatLabel(card({ info, format: "m4a", selectedFormat: "137" }))).toBe("M4A");
});

test("phone meta lines are shorter", () => {
  expect(phoneMeta(card({ info }))).toBe("Blender Foundation · 10:34");
  expect(phoneMeta(card({ url: "https://instagram.com/p/1", status: "downloading", waitUntil: 1, format: "m4a" }))).toBe("Instagram · M4A");
  expect(phoneMeta(card({ status: "error", jobState: "cancelled", progress: 63.2, format: "mkv" }))).toBe("Cancelled at 63% · MKV");
  expect(phoneMeta(card({ status: "done", format: "mp4", size: 18 * MB, strip: true }))).toBe("MP4 · 18 MB · no metadata");
});

test("stopped and done notes", () => {
  expect(stoppedNote(card({ status: "error", jobState: "cancelled", progress: 63, resumable: true }))).toBe(
    "Cancelled at 63%. Resume keeps the part already downloaded.",
  );
  expect(stoppedNote(card({ status: "error", jobState: "cancelled" }))).toBe("Download cancelled.");
  expect(stoppedNote(card({ status: "error", jobState: "failed", error: "This video is private" }))).toBe("This video is private");
  expect(doneNote(card({ filename: "Morning routine.mp4", size: 18 * MB }))).toBe("Morning routine.mp4 · 18 MB");
});

test("history notes, desktop and phone", () => {
  const now = new Date(2026, 10, 12, 12);
  const e = (over: Partial<HistoryEntry>): HistoryEntry => ({
    id: "h", url: "https://x", title: "T", format: "flac", format_id: null, size: 212 * MB, has_thumbnail: false,
    file_available: false, created_at: new Date(2026, 10, 9, 12).toISOString(), expires_at: null, playlist_item: null, ...over,
  });
  expect(historyNote(e({}), now)).toBe("FLAC · 212 MB · 3 days ago · file expired");
  expect(phoneHistoryNote(e({}), now)).toBe("FLAC · 3 days ago · expired");
  const today = e({ format: "mp4", size: 18 * MB, file_available: true, created_at: new Date(2026, 10, 12, 8).toISOString() });
  expect(historyNote(today, now)).toBe("MP4 · 18 MB · today");
  expect(phoneHistoryNote(today, now)).toBe("MP4 · 18 MB · today");
});

test("waitLeft rounds up and never goes below zero", () => {
  expect(waitLeft(card({ waitUntil: 24_000 }), 0)).toBe(24);
  expect(waitLeft(card({ waitUntil: 23_100 }), 0)).toBe(24);
  expect(waitLeft(card({ waitUntil: 1_000 }), 5_000)).toBe(0);
});
