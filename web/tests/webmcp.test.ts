import { beforeEach, expect, test } from "bun:test";
import tools from "../src/api/tools.gen.json";
import type { Job } from "../src/api/types.gen";
import { initialState, store } from "../src/public/store";
import { type ModelContextLike, registerWebMcp } from "../src/public/webmcp";

type Call = { path: string; method: string; body?: unknown };
let calls: Call[] = [];
let reply: (c: Call) => Response;

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const job = (over: Partial<Job> = {}): Job => ({
  id: "j1", url: "https://y/v", format: "mp4", format_id: null, title: null, thumbnail: null, state: "queued",
  filename: null, size: null, error: null, progress: null, speed: null, eta: null, created_at: "2026-01-01T00:00:00Z",
  expires_at: null, clip: null, strip_metadata: false, resumable: false, playlist_item: null, ...over,
});
const video = {
  kind: "video", title: "T", thumbnail: null, duration: 10, uploader: null,
  formats: [{ id: "137", label: "1080p", height: 1080, codec: null }, { id: "136", label: "720p", height: 720, codec: null }],
};

class FakeES {
  addEventListener() {}
  close() {}
}

type Registered = Parameters<NonNullable<ModelContextLike["registerTool"]>>[0];
function fakeContext(): { ctx: ModelContextLike; registered: Registered[] } {
  const registered: Registered[] = [];
  return { ctx: { registerTool: (t) => void registered.push(t) }, registered };
}
const call = async (t: Registered, input: Record<string, unknown>) => {
  const r = await t.execute(input);
  return { error: r.isError === true, body: JSON.parse(r.content[0].text) };
};

beforeEach(() => {
  calls = [];
  store.set(initialState());
  globalThis.EventSource = FakeES as never;
  globalThis.fetch = (async (input: string, init?: RequestInit) => {
    const c = { path: input, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : undefined };
    calls.push(c);
    return reply(c);
  }) as unknown as typeof fetch;
});

test("registers the six shared tools with their schemas", () => {
  const { ctx, registered } = fakeContext();
  expect(registerWebMcp(ctx)).toBe(true);
  expect(registered.map((t) => t.name)).toEqual([
    "probe_media", "start_download", "list_downloads", "get_download", "cancel_download", "get_transcript",
  ]);
  for (const t of registered) {
    const want = tools.find((x) => x.name === t.name);
    expect(t.description).toEqual(want?.description as string);
    expect(t.inputSchema).toEqual(want?.inputSchema);
  }
});

test("without a model context nothing is registered and nothing throws", () => {
  expect(registerWebMcp()).toBe(false);
  expect(registerWebMcp({})).toBe(false);
  expect(registerWebMcp({ registerTool: () => { throw new Error("boom"); } })).toBe(false);
});

test("provideContext is the fallback when registerTool is missing", () => {
  let given: unknown[] = [];
  expect(registerWebMcp({ provideContext: (c) => void (given = c.tools) })).toBe(true);
  expect(given).toHaveLength(6);
});

test("start_download shows a card, probes then creates the job with the given options", async () => {
  reply = (c) => (c.path === "/api/v1/probe" ? json(video) : c.path === "/api/v1/jobs" ? json(job({ title: "T" })) : json({}, 404));
  const { ctx, registered } = fakeContext();
  registerWebMcp(ctx);
  const start = registered.find((t) => t.name === "start_download") as Registered;
  const r = await call(start, { url: "https://y/v", format: "mkv", quality: "136", strip_metadata: true, clip: { start: 5, end: 9 } });
  expect(r.error).toBe(false);
  expect(r.body).toMatchObject({ id: "j1", state: "queued" });
  expect(calls.filter((c) => c.method === "POST").map((c) => c.path)).toEqual(["/api/v1/probe", "/api/v1/jobs"]);
  expect(calls.find((c) => c.path === "/api/v1/jobs")?.body).toMatchObject({
    url: "https://y/v", format: "mkv", format_id: "136", strip_metadata: true, clip: { start: 5, end: 9 },
  });
  const cards = store.get().cards;
  expect(cards).toHaveLength(1);
  expect(cards[0]).toMatchObject({ status: "downloading", jobId: "j1", url: "https://y/v" });
});

test("start_download rejects an unknown quality and reports API errors as results", async () => {
  reply = () => json(video);
  const { ctx, registered } = fakeContext();
  registerWebMcp(ctx);
  const start = registered.find((t) => t.name === "start_download") as Registered;
  const bad = await call(start, { url: "https://y/v", quality: "999" });
  expect(bad).toMatchObject({ error: true, body: { code: "invalid_request" } });
  expect(calls.some((c) => c.path === "/api/v1/jobs")).toBe(false);

  reply = () => json({ code: "rate_limited", message: "Slow down" }, 429);
  const limited = await call(start, { url: "https://y/other" });
  expect(limited.error).toBe(true);
  expect(limited.body.code).toBe("invalid_request");
  const direct = await call(registered.find((t) => t.name === "get_download") as Registered, { id: "j9" });
  expect(direct).toEqual({ error: true, body: { code: "rate_limited", message: "Slow down" } });
});

test("cancel_download goes through the card's cancel action", async () => {
  store.set({ cards: [{ key: "c1", url: "https://y/v", status: "downloading", jobId: "j1", jobState: "downloading" }] });
  reply = (c) => (c.path === "/api/v1/jobs/j1/cancel" ? json(job({ state: "cancelled" })) : json({}, 404));
  const { ctx, registered } = fakeContext();
  registerWebMcp(ctx);
  const r = await call(registered.find((t) => t.name === "cancel_download") as Registered, { id: "j1" });
  expect(r.body).toEqual({ id: "j1", state: "cancelled" });
  expect(calls.map((c) => [c.method, c.path])).toEqual([["POST", "/api/v1/jobs/j1/cancel"]]);
  expect(store.get().cards[0].jobState).toBe("cancelled");
});

test("get_download carries file_url only when done; list and transcript use the API", async () => {
  const { ctx, registered } = fakeContext();
  registerWebMcp(ctx);
  const get = registered.find((t) => t.name === "get_download") as Registered;
  reply = () => json(job({ state: "downloading", progress: 40 }));
  expect((await call(get, { id: "j1" })).body).not.toHaveProperty("file_url");
  reply = () => json(job({ state: "done" }));
  expect((await call(get, { id: "j1" })).body.file_url).toBe("/api/v1/jobs/j1/file");

  reply = () => json({ jobs: [job({ state: "done" })] });
  const list = await call(registered.find((t) => t.name === "list_downloads") as Registered, {});
  expect(list.body.downloads[0]).toMatchObject({ id: "j1", file_url: "/api/v1/jobs/j1/file" });

  reply = () => json({ language: "en", automatic: false, text: "hi" });
  const tr = await call(registered.find((t) => t.name === "get_transcript") as Registered, { url: "https://y/v", language: "en" });
  expect(tr.body.text).toBe("hi");
  expect(calls.at(-1)).toMatchObject({ path: "/api/v1/transcript", body: { url: "https://y/v", language: "en" } });
});
