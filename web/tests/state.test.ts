import { expect, test } from "bun:test";
import { ApiFailure } from "../src/api/client";
import { POLL_RETRY, expandPlaylists, pollJob } from "../src/state";
import type { Job, ProbeResponse } from "../src/api/types.gen";

const job = (state: Job["state"]): Job => ({
  id: "j1", url: "https://x", format: "mp4", format_id: null, title: null, thumbnail: null,
  state, filename: null, size: null, error: null, progress: null, speed: null, eta: null,
  created_at: "2026-01-01T00:00:00Z", expires_at: null,
});

/** A virtual clock: `sleep` advances it at once and records each wait. */
function clock() {
  let t = 0;
  const waits: number[] = [];
  return {
    waits,
    now: () => t,
    sleep: async (ms: number) => {
      waits.push(ms);
      t += ms;
    },
  };
}

test("expandPlaylists replaces a playlist by its entries", async () => {
  const probe = async (u: string): Promise<ProbeResponse> =>
    u.includes("list=")
      ? { kind: "playlist", title: "p", entries: ["https://1", "https://2"] }
      : { kind: "video", title: "v", thumbnail: "", duration: 1, uploader: "", formats: [] };
  expect(await expandPlaylists(["https://x?list=1", "https://y"], probe)).toEqual(["https://1", "https://2", "https://y"]);
});

test("pollJob retries network failures with backoff, then goes on", async () => {
  const c = clock();
  const answers: (Job | Error)[] = [new TypeError("offline"), new TypeError("offline"), job("downloading"), new TypeError("offline"), job("done")];
  const seen: string[] = [];
  const last = await pollJob("j1", async () => {
    const a = answers.shift()!;
    if (a instanceof Error) throw a;
    return a;
  }, (j) => seen.push(j.state), 1500, () => false, c);
  expect(last.state).toBe("done");
  expect(seen).toEqual(["downloading", "done"]);
  // A success resets the backoff.
  expect(c.waits).toEqual([1500, 3000, 1500, 1500]);
});

test("pollJob gives up on network failures after 2 minutes, every 10 s after 6 s", async () => {
  const c = clock();
  let calls = 0;
  const err = new TypeError("offline");
  const res = pollJob("j1", async () => { calls++; throw err; }, () => {}, 1500, () => false, c);
  await expect(res).rejects.toBe(err);
  expect(c.waits.slice(0, 4)).toEqual([1500, 3000, 6000, 10_000]);
  expect(c.waits.slice(3).every((w) => w === 10_000)).toBe(true);
  const total = c.waits.reduce((a, b) => a + b, 0);
  expect(total).toBeGreaterThanOrEqual(POLL_RETRY.giveUpMs);
  expect(total).toBeLessThan(POLL_RETRY.giveUpMs + 10_000);
  expect(calls).toBe(c.waits.length + 1);
});

test("pollJob fails at once on a 404", async () => {
  const c = clock();
  let calls = 0;
  const err = new ApiFailure("internal", "Not found", 404);
  const res = pollJob("j1", async () => { calls++; throw err; }, () => {}, 1500, () => false, c);
  await expect(res).rejects.toBe(err);
  expect(calls).toBe(1);
  expect(c.waits).toEqual([]);
});

test("pollJob retries a 5xx from a restarting server", async () => {
  const c = clock();
  const answers: (Job | Error)[] = [new ApiFailure("internal", "Bad gateway", 502), job("done")];
  const last = await pollJob("j1", async () => {
    const a = answers.shift()!;
    if (a instanceof Error) throw a;
    return a;
  }, () => {}, 1500, () => false, c);
  expect(last.state).toBe("done");
  expect(c.waits).toEqual([1500]);
});
