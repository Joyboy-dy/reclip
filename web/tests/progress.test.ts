import { expect, test } from "bun:test";
import { ApiFailure } from "../src/api/client";
import type { Job } from "../src/api/types.gen";
import { followJob } from "../src/progress";
import { formatProgress, progressWidth } from "../src/public/format";

const job = (state: Job["state"], progress: number | null = null): Job => ({
  id: "j1", url: "https://x", format: "mp4", format_id: null, title: null, thumbnail: null,
  state, filename: null, size: null, error: null, progress, speed: null, eta: null,
  created_at: "2026-01-01T00:00:00Z", expires_at: null, clip: null, strip_metadata: false, resumable: false, playlist_item: null,
});

class FakeES {
  static last: FakeES;
  listeners: Record<string, ((e: any) => void)[]> = {};
  closed = false;
  constructor(public url: string) { FakeES.last = this; }
  addEventListener(n: string, f: (e: any) => void) { (this.listeners[n] ??= []).push(f); }
  close() { this.closed = true; }
  emit(n: string, data?: unknown) {
    for (const f of this.listeners[n] ?? []) f({ data: JSON.stringify(data) });
  }
}

test("followJob delivers jobs and stops at the terminal one without polling", async () => {
  const seen: string[] = [];
  let polled = 0;
  followJob("j1", (j) => seen.push(j.state), {
    EventSource: FakeES as never,
    getJob: async () => { polled++; return job("done"); },
    intervalMs: 1,
  });
  expect(FakeES.last.url).toBe("/api/v1/jobs/j1/events");
  FakeES.last.emit("job", job("queued"));
  FakeES.last.emit("job", job("downloading", 10));
  FakeES.last.emit("job", job("done", 100));
  expect(FakeES.last.closed).toBe(true);
  FakeES.last.emit("error"); // stream closed by the server after the terminal job
  await new Promise((r) => setTimeout(r, 20));
  expect(seen).toEqual(["queued", "downloading", "done"]);
  expect(polled).toBe(0);
});

test("followJob falls back to polling on a real error", async () => {
  const seen: string[] = [];
  const states = ["downloading", "downloading", "done"] as const;
  let n = 0;
  followJob("j1", (j) => seen.push(j.state), {
    EventSource: FakeES as never,
    getJob: async () => job(states[n++]),
    intervalMs: 1,
  });
  FakeES.last.emit("job", job("queued"));
  FakeES.last.emit("error");
  await new Promise((r) => setTimeout(r, 50));
  expect(FakeES.last.closed).toBe(true);
  expect(seen).toEqual(["queued", "downloading", "downloading", "done"]);
});

test("the stop function silences further events and polling", async () => {
  const seen: string[] = [];
  const stop = followJob("j1", (j) => seen.push(j.state), {
    EventSource: FakeES as never,
    getJob: async () => job("downloading"),
    intervalMs: 1,
  });
  stop();
  FakeES.last.emit("job", job("downloading"));
  expect(seen).toEqual([]);
  expect(FakeES.last.closed).toBe(true);
});

test("formatProgress", () => {
  expect(formatProgress({ progress: 42.4, speed: 3_250_000, eta: 18 })).toBe("42% · 3.1 MB/s · 0:18 left");
  expect(formatProgress({ progress: 5, speed: 500_000, eta: 3725 })).toBe("5% · 488 KB/s · 1:02:05 left");
  expect(formatProgress({ progress: 50, speed: null, eta: null })).toBe("50%");
  expect(formatProgress({})).toBe("Starting...");
  expect(formatProgress({ progress: null, speed: null, eta: null, state: "queued" })).toBe("Queued");
  expect(formatProgress({ progress: 99.9, speed: 1, eta: 1, state: "processing" })).toBe("Processing");
});

test("progressWidth is clamped and full while processing", () => {
  expect(progressWidth({ progress: 42 })).toBe(42);
  expect(progressWidth({ progress: 140 })).toBe(100);
  expect(progressWidth({ progress: null })).toBe(0);
  expect(progressWidth({ progress: 10, state: "processing" })).toBe(100);
});

test("polling failure after an SSE error is reported once through onError", async () => {
  let calls = 0;
  const errors: unknown[] = [];
  followJob(
    "j1",
    () => {},
    {
      EventSource: FakeES as never,
      getJob: async () => { calls++; throw new ApiFailure("internal", "Not found", 404); },
      intervalMs: 1,
    },
    (e) => errors.push(e),
  );
  FakeES.last.emit("error");
  await new Promise((r) => setTimeout(r, 30));
  expect(errors.length).toBe(1);
  expect(calls).toBe(1);
});

test("onStreamEnd fires once when the stream closes, whatever the reason", async () => {
  const deps = { EventSource: FakeES as never, getJob: async () => job("done"), intervalMs: 1 };
  let ends = 0;
  followJob("j1", () => {}, deps, () => {}, () => ends++);
  FakeES.last.emit("job", job("downloading"));
  expect(ends).toBe(0);
  FakeES.last.emit("job", job("done"));
  FakeES.last.emit("error");
  expect(ends).toBe(1);

  ends = 0;
  const stop = followJob("j1", () => {}, deps, () => {}, () => ends++);
  FakeES.last.emit("error"); // falls back to polling: the connection is free
  expect(ends).toBe(1);
  stop();
  expect(ends).toBe(1);
  await new Promise((r) => setTimeout(r, 20));

  ends = 0;
  followJob("j1", () => {}, deps, () => {}, () => ends++)();
  expect(ends).toBe(1);
});
