import { expect, test } from "bun:test";
import type { Job } from "../src/api/types.gen";
import { applyJob, canRetry, cardFromJob, jobRequest } from "../src/state";

const job = (over: Partial<Job>): Job => ({
  id: "j1", url: "https://x", format: "mp4", format_id: "137", title: "T", thumbnail: "https://t/i.jpg",
  state: "queued", filename: null, size: null, error: null, progress: null, speed: null, eta: null,
  created_at: "2026-01-01T00:00:00Z", expires_at: null, clip: null, strip_metadata: false, resumable: false, playlist_item: null, ...over,
});

test("queued job becomes a downloading card", () => {
  const c = cardFromJob(job({}));
  expect(c).toMatchObject({ key: "j1", jobId: "j1", status: "downloading", jobState: "queued", title: "T", thumbnail: "https://t/i.jpg", url: "https://x" });
});

test("processing keeps live numbers", () => {
  const c = cardFromJob(job({ state: "processing", progress: 99.9, speed: 10, eta: 2 }));
  expect(c).toMatchObject({ status: "downloading", jobState: "processing", progress: 99.9, speed: 10, eta: 2 });
});

test("done job becomes a done card with its filename", () => {
  const c = cardFromJob(job({ state: "done", filename: "a.mp4", progress: 100 }));
  expect(c).toMatchObject({ status: "done", filename: "a.mp4", jobState: "done" });
});

test("failed job carries the friendly error", () => {
  const c = cardFromJob(job({ state: "failed", error: { code: "private", message: "m" } }));
  expect(c.status).toBe("error");
  expect(c.error).toBe("This video is private");
});

test("cancelled job reads as cancelled", () => {
  const c = cardFromJob(job({ state: "cancelled" }));
  expect(c.status).toBe("error");
  expect(c.error).toBe("Download cancelled");
});

test("restored failed job can be retried with its own format and quality", () => {
  const c = cardFromJob(job({ state: "failed", error: { code: "interrupted", message: "m" } }));
  expect(canRetry(c)).toBe(true);
  expect(jobRequest(c, { format: "mp3", strip: false })).toEqual({
    url: "https://x", format: "mp4", format_id: "137", title: "T", thumbnail: "https://t/i.jpg", strip_metadata: false,
  });
});

test("probed card follows the format toggle", () => {
  const c = { key: "k", url: "https://y", status: "error" as const, selectedFormat: "22",
    info: { title: "P", thumbnail: null, duration: null, uploader: null, formats: [] } };
  expect(jobRequest(c, { format: "mp3", strip: true })).toEqual({ url: "https://y", format: "mp3", title: "P", strip_metadata: true });
  expect(canRetry({ key: "n", url: "https://z", status: "error" })).toBe(false);
});

test("restored job keeps its own clip and metadata choice", () => {
  const c = cardFromJob(job({ state: "cancelled", clip: { start: 5, end: 30 }, strip_metadata: true, format: "mkv" }));
  expect(jobRequest(c, { format: "mp3", clip: { start: 1 }, strip: false })).toMatchObject({
    format: "mkv", format_id: "137", clip: { start: 5, end: 30 }, strip_metadata: true,
  });
});

test("probed card takes the page clip, metadata choice and container", () => {
  const c = { key: "k", url: "https://y", status: "ready" as const, selectedFormat: "22",
    info: { title: "P", thumbnail: null, duration: null, uploader: null, formats: [] } };
  expect(jobRequest(c, { format: "mov", clip: { start: 80, end: 125 }, strip: true })).toMatchObject({
    format: "mov", format_id: "22", clip: { start: 80, end: 125 }, strip_metadata: true,
  });
});

test("an item of a multi-video post sends its playlist_item", () => {
  const c = { key: "k", url: "https://x.com/a/status/1", status: "ready" as const, playlistItem: 2, title: "Video 2" };
  expect(canRetry(c)).toBe(true);
  expect(jobRequest(c, { format: "mp4", strip: false })).toMatchObject({ url: "https://x.com/a/status/1", format: "mp4", playlist_item: 2 });
});

test("a stopped job keeps the last progress seen and its resume flag", () => {
  const c = cardFromJob(job({ state: "downloading", progress: 63 }));
  applyJob(c, job({ state: "cancelled", progress: null, resumable: true }));
  expect(c).toMatchObject({ status: "error", progress: 63, resumable: true, jobState: "cancelled" });
});

test("a restored item of a multi-video post is retried with its playlist_item", () => {
  const c = cardFromJob(job({ state: "failed", error: { code: "interrupted", message: "m" }, playlist_item: 2 }));
  expect(c.playlistItem).toBe(2);
  expect(jobRequest(c, { format: "mp4", strip: false })).toMatchObject({ playlist_item: 2 });
});
