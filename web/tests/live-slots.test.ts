import { expect, test } from "bun:test";
import type { Job } from "../src/api/types.gen";
import { LiveSlots, MAX_LIVE, cappedFollower } from "../src/live-slots";

const job = (id: string, state: Job["state"]): Job => ({
  id, url: "https://x", format: "mp4", format_id: null, title: null, thumbnail: null,
  state, filename: null, size: null, error: null, progress: null, speed: null, eta: null,
  created_at: "2026-01-01T00:00:00Z", expires_at: null, clip: null, strip_metadata: false, resumable: false, playlist_item: null,
});

test("LiveSlots respects the cap and queues the rest in order", () => {
  const s = new LiveSlots(3);
  expect(["a", "b", "c", "d", "e"].map((k) => s.acquire(k))).toEqual([true, true, true, false, false]);
  expect(s.waitingKeys).toEqual(["d", "e"]);
  expect(s.acquire("a")).toBe(true); // already live
  expect(s.acquire("d")).toBe(false); // queued once only
  expect(s.waitingKeys).toEqual(["d", "e"]);
});

test("LiveSlots promotes the oldest waiting key on release", () => {
  const s = new LiveSlots(2);
  for (const k of ["a", "b", "c", "d"]) s.acquire(k);
  expect(s.release("b")).toBe("c");
  expect(s.isLive("c")).toBe(true);
  // Leaving the line frees nothing and is never promoted later.
  expect(s.release("d")).toBeUndefined();
  expect(s.release("a")).toBeUndefined();
  expect(s.isLive("d")).toBe(false);
  expect(s.release("nope")).toBeUndefined();
});

test("the default cap is 3", () => {
  expect(MAX_LIVE).toBe(3);
});

type LiveRun = { id: string; onJob: (j: Job) => void; onError: (e: unknown) => void; end: () => void; open: boolean };
type PollRun = { id: string; onJob: (j: Job) => void; onError: (e: unknown) => void; stopped: boolean };

/** Fakes behaving like followJob / pollFollow. */
function fakes(cap = 3) {
  const lives: LiveRun[] = [];
  const polls: PollRun[] = [];
  const follow = cappedFollower(
    (id, onJob, onError, onStreamEnd) => {
      const run: LiveRun = {
        id, onJob, onError, open: true,
        end: () => {
          if (run.open) { run.open = false; onStreamEnd(); }
        },
      };
      lives.push(run);
      return () => run.end();
    },
    (id, onJob, onError) => {
      const run: PollRun = { id, onJob, onError, stopped: false };
      polls.push(run);
      return () => { run.stopped = true; };
    },
    cap,
  );
  const liveIds = () => lives.filter((r) => r.open).map((r) => r.id);
  const pollIds = () => polls.filter((r) => !r.stopped).map((r) => r.id);
  return { follow, lives, polls, liveIds, pollIds };
}

const noop = () => {};

test("only `cap` jobs get a live stream, the others are polled", () => {
  const f = fakes(3);
  for (const id of ["j1", "j2", "j3", "j4", "j5"]) f.follow(id, noop, noop);
  expect(f.liveIds()).toEqual(["j1", "j2", "j3"]);
  expect(f.pollIds()).toEqual(["j4", "j5"]);
});

test("a terminal job frees its stream for the oldest polled job", () => {
  const f = fakes(3);
  const seen: string[] = [];
  for (const id of ["j1", "j2", "j3", "j4", "j5"]) f.follow(id, (j) => seen.push(`${j.id}:${j.state}`), noop);
  // followJob closes the stream, then delivers the terminal job.
  const j1 = f.lives[0]!;
  j1.end();
  j1.onJob(job("j1", "done"));
  expect(seen).toEqual(["j1:done"]);
  expect(f.liveIds()).toEqual(["j2", "j3", "j4"]);
  expect(f.pollIds()).toEqual(["j5"]);
});

test("a stream error frees the slot while the job goes on (polled by followJob)", () => {
  const f = fakes(1);
  const seen: string[] = [];
  f.follow("a", (j) => seen.push(`${j.id}:${j.state}`), noop);
  f.follow("b", noop, noop);
  f.lives[0]!.end(); // error: EventSource closed, followJob falls back to polling
  expect(f.liveIds()).toEqual(["b"]);
  expect(f.pollIds()).toEqual([]);
  f.lives[0]!.onJob(job("a", "downloading"));
  expect(seen).toEqual(["a:downloading"]);
});

test("stopping a live follow (Remove) promotes the next one", () => {
  const f = fakes(2);
  const stops = ["a", "b", "c"].map((id) => f.follow(id, noop, noop));
  stops[0]!();
  expect(f.liveIds()).toEqual(["b", "c"]);
  expect(f.pollIds()).toEqual([]);
  stops[0]!(); // twice is harmless
  expect(f.liveIds()).toEqual(["b", "c"]);
});

test("a polled job leaves the line when it finishes, fails or is stopped", () => {
  const f = fakes(1);
  const errors: unknown[] = [];
  f.follow("a", noop, noop);
  f.follow("b", noop, noop);
  f.follow("c", noop, (e) => errors.push(e));
  const stopD = f.follow("d", noop, noop);
  f.follow("e", noop, noop);
  f.polls[0]!.onJob(job("b", "cancelled")); // b terminal while polled
  f.polls[1]!.onError(new Error("gone")); // c gave up
  stopD(); // d removed
  expect(errors.length).toBe(1);
  f.lives[0]!.end(); // a's stream ends: e is next, not b, c or d
  expect(f.liveIds()).toEqual(["e"]);
});

test("a promoted job stops being polled and gets a live stream", () => {
  const f = fakes(1);
  f.follow("a", noop, noop);
  f.follow("b", noop, noop);
  expect(f.pollIds()).toEqual(["b"]);
  f.lives[0]!.end();
  expect(f.polls[0]!.stopped).toBe(true);
  expect(f.liveIds()).toEqual(["b"]);
});
