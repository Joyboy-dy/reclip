import { beforeEach, expect, test } from "bun:test";
import type { Job, ProbeResponse } from "../src/api/types.gen";
import { download, fetchLinks, resume } from "../src/public/downloads";
import { clean, downloadAgain, loadHistory, removeEntry } from "../src/public/history";
import { findCard, initialState, setOptions, store } from "../src/public/store";

type Call = { path: string; method: string; body?: unknown };
let calls: Call[] = [];
let reply: (c: Call) => Response;

const json = (body: unknown, status = 200, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json", ...headers } });
const pending = () => json({ code: "internal", message: "Not implemented yet" }, 501);

const job = (over: Partial<Job> = {}): Job => ({
  id: "j1", url: "https://y/v", format: "mp4", format_id: null, title: null, thumbnail: null, state: "queued",
  filename: null, size: null, error: null, progress: null, speed: null, eta: null, created_at: "2026-01-01T00:00:00Z",
  expires_at: null, clip: null, strip_metadata: false, resumable: false, playlist_item: null, ...over,
});
const video = (title: string): ProbeResponse => ({
  kind: "video", title, thumbnail: null, duration: 10, uploader: null,
  formats: [{ id: "137", label: "1080p", height: 1080, codec: null }, { id: "136", label: "720p", height: 720, codec: null }],
});

class FakeES {
  addEventListener() {}
  close() {}
}

beforeEach(() => {
  calls = [];
  store.set(initialState());
  globalThis.EventSource = FakeES as never;
  globalThis.fetch = (async (input: string, init?: RequestInit) => {
    const call = { path: input, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : undefined };
    calls.push(call);
    return reply(call);
  }) as unknown as typeof fetch;
});

test("a playlist is expanded: post items are ready at once, other entries are probed", async () => {
  reply = (c) => {
    const url = (c.body as { url: string }).url;
    if (url === "https://x.com/a/status/1") {
      return json({ kind: "playlist", title: "post", entries: [
        { url: "https://x.com/a/status/1", title: null, playlist_item: 1 },
        { url: "https://x.com/a/status/1", title: "Second", playlist_item: 2 },
      ] });
    }
    if (url === "https://y/list") return json({ kind: "playlist", title: "p", entries: [{ url: "https://y/1", title: null, playlist_item: null }] });
    return json(video(`probed ${url}`));
  };
  expect(await fetchLinks(["https://x.com/a/status/1", "https://y/list"])).toBe(true);
  const cards = store.get().cards;
  expect(cards.map((c) => [c.status, c.title ?? c.info?.title, c.playlistItem])).toEqual([
    ["ready", "Video 1", 1],
    ["ready", "Second", 2],
    ["ready", "probed https://y/1", undefined],
  ]);
  expect(cards[2].selectedFormat).toBe("137");
  expect(store.get().busy).toBe(false);
});

test("download sends the page options: container, quality, clip and metadata", async () => {
  reply = (c) => (c.path === "/api/v1/probe" ? json(video("v")) : json(job({ format: "mkv", format_id: "136", strip_metadata: true })));
  await fetchLinks(["https://y/v"]);
  const key = store.get().cards[0].key;
  store.set((s) => ({ cards: s.cards.map((c) => ({ ...c, selectedFormat: "136" })) }));
  setOptions({ container: "mkv", clipOn: true, clipFrom: "1:20", clipTo: "2:05", strip: true });
  await download(key);
  const create = calls.find((c) => c.path === "/api/v1/jobs" && c.method === "POST");
  expect(create?.body).toEqual({ url: "https://y/v", format: "mkv", format_id: "136", title: "v", clip: { start: 80, end: 125 }, strip_metadata: true });
  expect(findCard(key)).toMatchObject({ status: "downloading", jobId: "j1", strip: true });
});

test("invalid clip times keep the card ready and say why", async () => {
  reply = () => json(video("v"));
  await fetchLinks(["https://y/v"]);
  const key = store.get().cards[0].key;
  setOptions({ clipOn: true, clipFrom: "2:00", clipTo: "1:00" });
  await download(key);
  expect(findCard(key)).toMatchObject({ status: "ready", error: "Clip end must come after its start" });
  expect(calls.some((c) => c.path === "/api/v1/jobs")).toBe(false);
});

test("a rate-limited creation waits, then retries by itself", async () => {
  let tries = 0;
  reply = (c) => {
    if (c.path === "/api/v1/probe") return json(video("v"));
    return tries++ === 0 ? json({ code: "rate_limited", message: "slow" }, 429, { "Retry-After": "0" }) : json(job());
  };
  await fetchLinks(["https://y/v"]);
  const key = store.get().cards[0].key;
  await download(key);
  expect(tries).toBe(2);
  expect(findCard(key)).toMatchObject({ status: "downloading", jobId: "j1", waitUntil: undefined });
});

test("a refused resume is shown on the card, which stays stopped", async () => {
  store.set({ cards: [{ key: "j1", url: "https://y/v", status: "error", jobId: "j1", jobState: "cancelled", resumable: true }] });
  reply = pending;
  await resume("j1");
  expect(calls[0]).toMatchObject({ path: "/api/v1/jobs/j1/resume", method: "POST" });
  expect(findCard("j1")).toMatchObject({ status: "error", actionError: "Not implemented yet" });
});

test("history: a failed load and a failed action are reported, the list is kept", async () => {
  reply = pending;
  await loadHistory();
  expect(store.get().history).toEqual({ status: "error", error: "Not implemented yet" });

  const entry = { id: "h1", url: "https://y/v", title: "T", format: "mp4" as const, format_id: null, size: 1, has_thumbnail: false,
    file_available: true, created_at: "2026-01-01T00:00:00Z", expires_at: null, playlist_item: null };
  reply = () => json({ entries: [entry] });
  await loadHistory();
  expect(store.get().history).toEqual({ status: "ready", entries: [entry] });

  reply = pending;
  await removeEntry("h1");
  expect(store.get().history).toEqual({ status: "ready", entries: [entry], error: "Not implemented yet" });

  reply = (c) => (c.path === "/api/v1/history/clean" ? json({ removed: 1 }) : json({ entries: [] }));
  await clean({ mode: "older_than", days: 7 });
  expect(calls.find((c) => c.path === "/api/v1/history/clean")?.body).toEqual({ mode: "older_than", days: 7 });
  expect(store.get().history).toEqual({ status: "ready", entries: [] });
});

test("download again sends the entry's format, quality and playlist_item", async () => {
  reply = () => json(job());
  downloadAgain({ id: "h1", url: "https://x.com/a/status/1", title: "T", format: "mkv", format_id: "136", size: 1,
    has_thumbnail: false, file_available: false, created_at: "2026-01-01T00:00:00Z", expires_at: null, playlist_item: 2 });
  await new Promise((r) => setTimeout(r, 0));
  const create = calls.find((c) => c.path === "/api/v1/jobs" && c.method === "POST");
  expect(create?.body).toMatchObject({ url: "https://x.com/a/status/1", format: "mkv", format_id: "136", playlist_item: 2 });
});

test("an invalid page clip does not block a card that keeps its own options", async () => {
  setOptions({ clipOn: true, clipFrom: "2:00", clipTo: "1:00" });
  store.set({ cards: [{ key: "j0", url: "https://y/v", status: "error", jobId: "j0", jobState: "failed", format: "mkv",
    selectedFormat: "136", clip: { start: 5, end: 30 }, strip: true }] });
  reply = () => json(job());
  await download("j0");
  const create = calls.find((c) => c.path === "/api/v1/jobs" && c.method === "POST");
  expect(create?.body).toMatchObject({ format: "mkv", format_id: "136", clip: { start: 5, end: 30 }, strip_metadata: true });
  expect(findCard("j0")).toMatchObject({ status: "downloading", jobId: "j1", error: undefined });
});
