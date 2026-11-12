import { expect, test } from "bun:test";
import type { Job } from "../src/api/types.gen";
import { canRetry, cardFromJob, jobRequest } from "../src/state";

const job = (over: Partial<Job>): Job => ({
  id: "j1", url: "https://x", format: "mp4", format_id: "137", title: "T", thumbnail: "https://t/i.jpg",
  state: "queued", filename: null, size: null, error: null, progress: null, speed: null, eta: null,
  created_at: "2026-01-01T00:00:00Z", expires_at: null, ...over,
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
  expect(jobRequest(c, "mp3")).toEqual({ url: "https://x", format: "mp4", format_id: "137", title: "T", thumbnail: "https://t/i.jpg" });
});

test("probed card follows the format toggle", () => {
  const c = { key: "k", url: "https://y", status: "error" as const, selectedFormat: "22",
    info: { title: "P", thumbnail: null, duration: null, uploader: null, formats: [] } };
  expect(jobRequest(c, "mp3")).toEqual({ url: "https://y", format: "mp3", format_id: undefined, title: "P", thumbnail: undefined });
  expect(canRetry({ key: "n", url: "https://z", status: "error" })).toBe(false);
});
