import { expect, test } from "bun:test";
import { cleanHistory, historyThumbUrl, listHistory, probe, removeHistory, resumeJob } from "../src/api/client";

test("client throws ApiFailure with the server code", async () => {
  globalThis.fetch = (async () =>
    new Response(JSON.stringify({ code: "bot_check", message: "m" }), { status: 422 })) as unknown as typeof fetch;
  await expect(probe("https://x")).rejects.toMatchObject({ code: "bot_check", status: 422 });
});

test("retry_after_is_parsed", async () => {
  const reply = (retryAfter: string | null) => {
    globalThis.fetch = (async () =>
      new Response(JSON.stringify({ code: "rate_limited", message: "slow down" }), {
        status: 429,
        headers: retryAfter === null ? {} : { "Retry-After": retryAfter },
      })) as unknown as typeof fetch;
    return probe("https://x");
  };
  await expect(reply("17")).rejects.toMatchObject({ code: "rate_limited", retryAfter: 17 });
  await expect(reply("Wed, 21 Oct 2026 07:28:00 GMT")).rejects.toMatchObject({ retryAfter: undefined });
  await expect(reply("-3")).rejects.toMatchObject({ retryAfter: undefined });
  await expect(reply(null)).rejects.toMatchObject({ retryAfter: undefined });
});

test("history and resume routes", async () => {
  const seen: string[] = [];
  globalThis.fetch = (async (path: string, init?: RequestInit) => {
    seen.push(`${init?.method ?? "GET"} ${path} ${init?.body ?? ""}`.trim());
    return new Response(JSON.stringify(path === "/api/v1/history" ? { entries: [] } : { removed: 2 }), { status: 200 });
  }) as unknown as typeof fetch;
  expect(await listHistory()).toEqual([]);
  await removeHistory("a/b");
  expect(await cleanHistory({ mode: "all" })).toEqual({ removed: 2 });
  await resumeJob("j1");
  expect(seen).toEqual([
    "GET /api/v1/history",
    "DELETE /api/v1/history/a%2Fb",
    'POST /api/v1/history/clean {"mode":"all"}',
    "POST /api/v1/jobs/j1/resume {}",
  ]);
  expect(historyThumbUrl("h 1")).toBe("/api/v1/history/h%201/thumb");
});
