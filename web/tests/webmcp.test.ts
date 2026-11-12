import { afterEach, beforeEach, expect, test } from "bun:test";
import tools from "../src/api/tools.gen.json";
import type { Job } from "../src/api/types.gen";
import { download, fetchLinks, restore } from "../src/public/downloads";
import { initialState, store } from "../src/public/store";
import { type ModelContextLike, registerWebMcp, registerWhenPublic } from "../src/public/webmcp";

type Call = { path: string; method: string; body?: any };
let calls: Call[] = [];
let reply: (c: Call) => Response | Promise<Response>;

const json = (body: unknown, status = 200, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json", ...headers } });
const job = (over: Partial<Job> = {}): Job => ({
  id: "j1", url: "https://y/v", format: "mp4", format_id: null, title: null, thumbnail: null, state: "queued",
  filename: null, size: null, error: null, progress: null, speed: null, eta: null, created_at: "2026-01-01T00:00:00Z",
  expires_at: null, clip: null, strip_metadata: false, resumable: false, playlist_item: null, ...over,
});
const video = {
  kind: "video", title: "T", thumbnail: null, duration: 10, uploader: null,
  formats: [{ id: "137", label: "1080p", height: 1080, codec: null }, { id: "136", label: "720p", height: 720, codec: null }],
};
const post = {
  kind: "playlist", title: "post",
  entries: [
    { url: "https://x/p", title: null, playlist_item: 1 },
    { url: "https://x/p", title: "Second", playlist_item: 2 },
  ],
};

class FakeES {
  static all: FakeES[] = [];
  listeners = new Map<string, (e: { data: string }) => void>();
  constructor() {
    FakeES.all.push(this);
  }
  addEventListener(name: string, fn: (e: { data: string }) => void) {
    this.listeners.set(name, fn);
  }
  close() {}
}

type Registered = Parameters<NonNullable<ModelContextLike["registerTool"]>>[0];
function setup(): Record<string, Registered> {
  const registered: Registered[] = [];
  registerWebMcp({ registerTool: (t) => void registered.push(t) });
  return Object.fromEntries(registered.map((t) => [t.name, t]));
}
const call = async (t: Registered, input: Record<string, unknown>) => {
  const r = await t.execute(input);
  return { error: r.isError === true, body: JSON.parse(r.content[0].text) };
};
const posts = () => calls.filter((c) => c.method === "POST").map((c) => c.path);

beforeEach(() => {
  calls = [];
  FakeES.all = [];
  store.set(initialState());
  globalThis.EventSource = FakeES as never;
  globalThis.fetch = (async (input: string, init?: RequestInit) => {
    const c = { path: input, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : undefined };
    calls.push(c);
    return reply(c);
  }) as unknown as typeof fetch;
});
afterEach(() => {
  delete (globalThis as { document?: unknown }).document;
  delete (navigator as { modelContext?: unknown }).modelContext;
});

test("registers the six shared tools with their schemas", () => {
  const t = setup();
  expect(Object.keys(t)).toEqual(["probe_media", "start_download", "list_downloads", "get_download", "cancel_download", "get_transcript"]);
  for (const want of tools) {
    expect(t[want.name].description).toEqual(want.description);
    expect(t[want.name].inputSchema).toEqual(want.inputSchema);
  }
});

test("without a model context nothing is registered and nothing throws", () => {
  expect(registerWebMcp()).toBe(false);
  expect(registerWebMcp({})).toBe(false);
  expect(registerWebMcp({ registerTool: () => { throw new Error("boom"); } })).toBe(false);
});

test("one failing registration does not hide the other tools; provideContext is the fallback", () => {
  const names: string[] = [];
  const ctx = { registerTool: (t: Registered) => { if (t.name === "probe_media") throw new Error("dup"); names.push(t.name); } };
  expect(registerWebMcp(ctx)).toBe(true);
  expect(names).toHaveLength(5);
  let given: unknown[] = [];
  expect(registerWebMcp({ provideContext: (c) => void (given = c.tools) })).toBe(true);
  expect(given).toHaveLength(6);
});

test("document.modelContext is preferred over navigator.modelContext", () => {
  const seen: string[] = [];
  const mk = (who: string) => ({ registerTool: () => void seen.push(who) });
  (globalThis as { document?: unknown }).document = { modelContext: mk("document") };
  Object.defineProperty(navigator, "modelContext", { value: mk("navigator"), configurable: true });
  expect(registerWebMcp()).toBe(true);
  expect(new Set(seen)).toEqual(new Set(["document"]));
});

test("start_download adds a visible card, probes then creates the job with the given options", async () => {
  reply = (c) => (c.path === "/api/v1/probe" ? json(video) : c.path === "/api/v1/jobs" ? json(job({ title: "T" })) : json({}, 404));
  const r = await call(setup().start_download, { url: "https://y/v", format: "mkv", quality: "136", strip_metadata: true, clip: { start: 5, end: 9 } });
  expect(r.error).toBe(false);
  expect(r.body).toMatchObject({ id: "j1", state: "queued" });
  expect(posts()).toEqual(["/api/v1/probe", "/api/v1/jobs"]);
  expect(calls.find((c) => c.path === "/api/v1/jobs")?.body).toMatchObject({
    url: "https://y/v", format: "mkv", format_id: "136", strip_metadata: true, clip: { start: 5, end: 9 },
  });
  expect(store.get().cards).toHaveLength(1);
  expect(store.get().cards[0]).toMatchObject({ status: "downloading", jobId: "j1", url: "https://y/v", autoSave: false });
  expect(store.get().busy).toBe(false);
});

test("start_download keeps the existing cards and busy untouched", async () => {
  store.set({ cards: [{ key: "u1", url: "https://u/1", status: "ready" }, { key: "u2", url: "https://u/2", status: "done", jobId: "ju" }] });
  reply = (c) => (c.path === "/api/v1/probe" ? json(video) : json(job()));
  await call(setup().start_download, { url: "https://y/v" });
  expect(store.get().cards.map((c) => c.key).slice(1)).toEqual(["u1", "u2"]);
  expect(store.get().cards).toHaveLength(3);
  expect(store.get().busy).toBe(false);
});

test("a multi-video link yields only the requested item, never extra Ready cards", async () => {
  reply = (c) => (c.path === "/api/v1/probe" ? json(post) : json(job({ playlist_item: 2 })));
  const t = setup();
  const lost = await call(t.start_download, { url: "https://x/p" });
  expect(lost).toMatchObject({ error: true, body: { code: "invalid_request" } });
  expect(lost.body.message).toContain("playlist_item");
  expect(store.get().cards).toHaveLength(0);
  const r = await call(t.start_download, { url: "https://x/p", playlist_item: 2 });
  expect(r.error).toBe(false);
  expect(store.get().cards).toHaveLength(1);
  expect(store.get().cards[0]).toMatchObject({ playlistItem: 2, jobId: "j1" });
  expect(calls.find((c) => c.path === "/api/v1/jobs")?.body.playlist_item).toBe(2);
  const none = await call(t.start_download, { url: "https://x/p", playlist_item: 9 });
  expect(none.body.code).toBe("invalid_request");
  expect(store.get().cards).toHaveLength(1);
});

test("concurrent start_download calls each get their own card and job", async () => {
  reply = (c) => (c.path === "/api/v1/probe" ? json(video) : json(job({ id: `job-${c.body.url}`, url: c.body.url })));
  const t = setup();
  const [a, b] = await Promise.all([
    call(t.start_download, { url: "https://y/a" }),
    call(t.start_download, { url: "https://y/b" }),
  ]);
  expect([a.error, b.error]).toEqual([false, false]);
  expect(a.body.id).toBe("job-https://y/a");
  expect(b.body.id).toBe("job-https://y/b");
  const cards = store.get().cards;
  expect(cards.map((c) => [c.url, c.jobId]).sort()).toEqual([["https://y/a", "job-https://y/a"], ["https://y/b", "job-https://y/b"]]);
});

test("a user Fetch during an agent probe keeps the agent's card and the user's", async () => {
  let release = () => {};
  const gate = new Promise<void>((r) => (release = r));
  reply = async (c) => {
    if (c.path === "/api/v1/probe" && c.body.url === "https://y/agent") await gate;
    return c.path === "/api/v1/probe" ? json(video) : json(job({ url: c.body.url }));
  };
  const started = call(setup().start_download, { url: "https://y/agent" });
  await Bun.sleep(10);
  expect(await fetchLinks(["https://y/user"])).toBe(true);
  release();
  expect((await started).error).toBe(false);
  const cards = store.get().cards;
  expect(cards.map((c) => c.url).sort()).toEqual(["https://y/agent", "https://y/user"]);
  expect(cards.find((c) => c.url === "https://y/agent")).toMatchObject({ jobId: "j1", autoSave: false });
  expect(cards.find((c) => c.url === "https://y/user")?.status).toBe("ready");
});

test("inputs are checked against the tool schemas", async () => {
  reply = () => json(video);
  const t = setup();
  const u = "https://y/v";
  const bad: [string, Record<string, unknown>][] = [
    ["start_download", { url: u, format: "avi" }],
    ["start_download", { url: u, clip: { start: -1 } }],
    ["start_download", { url: u, clip: { start: 5, end: 3 } }],
    ["start_download", { url: u, clip: { start: 1, end: "9" } }],
    ["start_download", { url: u, clip: { start: Number.NaN } }],
    ["start_download", { url: u, clip: {} }],
    ["start_download", { url: u, strip_metadata: "yes" }],
    ["start_download", { url: u, playlist_item: 0 }],
    ["start_download", { url: u, playlist_item: 1.5 }],
    ["start_download", { url: u, format: "mp3", quality: "137" }],
    ["start_download", { url: u, extra: 1 }],
    ["start_download", { url: "not a url" }],
    ["start_download", {}],
    ["probe_media", { url: u, extra: 1 }],
    ["get_download", {}],
    ["cancel_download", { id: 5 }],
    ["get_transcript", { url: u, language: "en us!" }],
  ];
  for (const [name, input] of bad) {
    const r = await call(t[name], input);
    expect([name, input, r.error, r.body.code]).toEqual([name, input, true, "invalid_request"]);
  }
  expect(calls).toHaveLength(0);
  expect(store.get().cards).toHaveLength(0);
});

test("an agent's job does not save the file in the browser; the agent gets download_url", async () => {
  let clicks = 0;
  (globalThis as { document?: unknown }).document = {
    createElement: () => ({ click: () => clicks++, remove() {} }),
    body: { appendChild() {} },
  };
  const done = job({ state: "done", filename: "t.mp4" });
  reply = (c) => (c.path === "/api/v1/probe" ? json(video) : c.method === "POST" ? json(job()) : json(done));
  const t = setup();
  const started = await call(t.start_download, { url: u1 });
  expect(started.body.id).toBe("j1");
  for (const es of FakeES.all) es.listeners.get("job")?.({ data: JSON.stringify(done) });
  for (let i = 0; i < 50 && store.get().cards[0].status !== "done"; i++) await Bun.sleep(10);
  expect(store.get().cards[0].status).toBe("done");
  expect(clicks).toBe(0);
  expect((await call(t.get_download, { id: "j1" })).body.download_url).toBe("/api/v1/jobs/j1/file");
});
const u1 = "https://y/v";

test("cancel_download goes through the card's cancel action and keeps error codes", async () => {
  store.set({ cards: [{ key: "c1", url: u1, status: "downloading", jobId: "j1", jobState: "downloading" }] });
  reply = (c) => (c.path === "/api/v1/jobs/j1/cancel" ? json(job({ state: "cancelled" })) : json({}, 404));
  const t = setup();
  const r = await call(t.cancel_download, { id: "j1" });
  expect(r.body).toEqual({ id: "j1", state: "cancelled" });
  expect(calls.map((c) => [c.method, c.path])).toEqual([["POST", "/api/v1/jobs/j1/cancel"]]);
  expect(store.get().cards[0].jobState).toBe("cancelled");

  store.set({ cards: [{ key: "c2", url: u1, status: "downloading", jobId: "j2", jobState: "downloading" }] });
  reply = () => json({ code: "forbidden", message: "Not yours" }, 403);
  expect((await call(t.cancel_download, { id: "j2" })).body.code).toBe("forbidden");
});

test("server error codes are kept", async () => {
  const t = setup();
  reply = () => json({ code: "rate_limited", message: "Slow down" }, 429);
  expect((await call(t.start_download, { url: u1 })).body.code).toBe("rate_limited");
  expect((await call(t.probe_media, { url: u1 })).body.code).toBe("rate_limited");
  expect(await call(t.get_download, { id: "j9" })).toEqual({ error: true, body: { code: "rate_limited", message: "Slow down" } });
  reply = (c) => (c.path === "/api/v1/probe" ? json(video) : json({ code: "forbidden", message: "No jobs" }, 403));
  expect((await call(t.start_download, { url: "https://y/other" })).body.code).toBe("forbidden");
});

test("get_download carries download_url only when done; list and transcript use the API", async () => {
  const t = setup();
  reply = () => json(job({ state: "downloading", progress: 40 }));
  expect((await call(t.get_download, { id: "j1" })).body).not.toHaveProperty("download_url");
  reply = () => json(job({ state: "done" }));
  expect((await call(t.get_download, { id: "j1" })).body.download_url).toBe("/api/v1/jobs/j1/file");

  store.set({ cards: [{ key: "c9", url: "https://y/w", status: "ready" }, { key: "c8", url: "https://y/e", status: "info-error" }] });
  reply = () => json({ jobs: [job({ state: "done" })] });
  const list = await call(t.list_downloads, {});
  expect(list.body.jobs.map((d: { id: string; state: string }) => [d.id, d.state])).toEqual([["c9", "pending"], ["j1", "done"]]);
  expect(list.body.jobs[1].download_url).toBe("/api/v1/jobs/j1/file");

  reply = () => json({ language: "en", automatic: false, text: "hi" });
  const tr = await call(t.get_transcript, { url: u1, language: "en" });
  expect(tr.body.text).toBe("hi");
  expect(calls.at(-1)).toMatchObject({ path: "/api/v1/transcript", body: { url: u1, language: "en" } });
});

test("a rate-limited creation returns a waiting card at once", async () => {
  reply = (c) => (c.path === "/api/v1/probe" ? json(video) : json({ code: "rate_limited", message: "Wait" }, 429, { "Retry-After": "30" }));
  const t = setup();
  const r = await call(t.start_download, { url: u1 });
  expect(r.error).toBe(false);
  expect(r.body).toMatchObject({ state: "waiting", id: store.get().cards[0].key });
  const got = await call(t.get_download, { id: r.body.id });
  expect(got.body).toMatchObject({ state: "pending", id: r.body.id });
  expect((await call(t.cancel_download, { id: r.body.id })).body.state).toBe("cancelled");
  expect(store.get().cards).toHaveLength(0);
});

// A fake sessionStorage (Bun has none) and a counting document for saveFile.
function fakeBrowser() {
  const mem = new Map<string, string>();
  (globalThis as { sessionStorage?: unknown }).sessionStorage = {
    getItem: (k: string) => mem.get(k) ?? null,
    setItem: (k: string, v: string) => void mem.set(k, v),
  };
  const state = { clicks: 0 };
  (globalThis as { document?: unknown }).document = {
    createElement: () => ({ click: () => state.clicks++, remove() {} }),
    body: { appendChild() {} },
  };
  return state;
}
const finish = async (id: string, done: Job) => {
  for (const es of FakeES.all) es.listeners.get("job")?.({ data: JSON.stringify(done) });
  for (let i = 0; i < 50 && store.get().cards.find((c) => c.key === id)?.status !== "done"; i++) await Bun.sleep(10);
};

test("after a reload an agent's download is still not saved; a user's restored card is", async () => {
  const saved = fakeBrowser();
  try {
    reply = (c) => (c.path === "/api/v1/probe" ? json(video) : json(job()));
    await call(setup().start_download, { url: u1 });
    store.set(initialState());
    FakeES.all = [];
    const done = job({ state: "done", filename: "t.mp4" });
    reply = (c) => (c.path === "/api/v1/jobs" ? json({ jobs: [job()] }) : json(done));
    await restore();
    expect(store.get().cards.map((c) => [c.key, c.autoSave])).toEqual([["j1", false]]);
    await finish("j1", done);
    expect(saved.clicks).toBe(0);

    store.set(initialState());
    FakeES.all = [];
    const mine = job({ id: "ju", url: "https://u/1" });
    reply = (c) => (c.path === "/api/v1/jobs" ? json({ jobs: [mine] }) : json(done));
    await restore();
    expect(store.get().cards[0].autoSave).toBeUndefined();
    await finish("ju", { ...done, id: "ju" });
    expect(saved.clicks).toBe(1);
  } finally {
    delete (globalThis as { sessionStorage?: unknown }).sessionStorage;
  }
});

test("a broken sessionStorage never breaks starting or restoring", async () => {
  (globalThis as { sessionStorage?: unknown }).sessionStorage = {
    getItem: () => { throw new Error("blocked"); },
    setItem: () => { throw new Error("blocked"); },
  };
  try {
    reply = (c) => (c.path === "/api/v1/probe" ? json(video) : json(job()));
    expect((await call(setup().start_download, { url: u1 })).error).toBe(false);
    reply = () => json({ jobs: [job()] });
    store.set(initialState());
    await restore();
    expect(store.get().cards).toHaveLength(1);
  } finally {
    delete (globalThis as { sessionStorage?: unknown }).sessionStorage;
  }
});

test("cancel_download removes a card without a job only when it is an agent card", async () => {
  const t = setup();
  store.set({ cards: [{ key: "mine", url: u1, status: "ready" }, { key: "agent", url: u1, status: "ready", autoSave: false }] });
  const mine = await call(t.cancel_download, { id: "mine" });
  expect(mine).toEqual({ error: true, body: { code: "invalid_request", message: "Not a download" } });
  expect(store.get().cards.map((c) => c.key)).toEqual(["mine", "agent"]);
  expect((await call(t.cancel_download, { id: "agent" })).body).toEqual({ id: "agent", state: "cancelled" });
  expect(store.get().cards.map((c) => c.key)).toEqual(["mine"]);
});

test("retrying a failed agent card reuses the agent's options, not the page's", async () => {
  fakeBrowser();
  try {
    reply = (c) => (c.path === "/api/v1/probe" ? json(video) : json({ code: "internal", message: "x" }, 500));
    await call(setup().start_download, { url: u1, format: "mkv", strip_metadata: true, clip: { start: 5, end: 9 } });
    expect(store.get().cards[0].status).toBe("error");
    store.set((s) => ({ options: { ...s.options, media: "audio", audio: "mp3", strip: false, clipOn: false } }));
    calls = [];
    reply = () => json(job());
    await download(store.get().cards[0].key);
    expect(calls.find((c) => c.path === "/api/v1/jobs")?.body).toMatchObject({
      format: "mkv", strip_metadata: true, clip: { start: 5, end: 9 },
    });
  } finally {
    delete (globalThis as { sessionStorage?: unknown }).sessionStorage;
  }
});

test("tools are registered only when public_web is on", () => {
  const seen: string[] = [];
  (globalThis as { document?: unknown }).document = { modelContext: { registerTool: (t: Registered) => void seen.push(t.name) } };
  expect(registerWhenPublic({ public_web: false })).toBe(false);
  expect(seen).toHaveLength(0);
  expect(registerWhenPublic({ public_web: true })).toBe(true);
  expect(seen).toHaveLength(6);
  delete (globalThis as { document?: unknown }).document;
  expect(registerWhenPublic({ public_web: true })).toBe(false);
});
