import { expect, test } from "bun:test";
import { getConfig, listHistory, startBootstrap } from "../src/api/client";

test("requests wait for the bootstrap response that sets the visitor cookie", async () => {
  const order: string[] = [];
  let releaseConfig: () => void = () => {};
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const path = String(input);
    order.push(`start ${path}`);
    if (path === "/api/v1/config") {
      await new Promise<void>((r) => (releaseConfig = r));
      order.push("config done");
      return new Response(JSON.stringify({}), { status: 200 });
    }
    return new Response(JSON.stringify({ entries: [] }), { status: 200 });
  }) as unknown as typeof fetch;

  const boot = startBootstrap(getConfig);
  const history = listHistory();
  await new Promise((r) => setTimeout(r, 10));
  expect(order).toEqual(["start /api/v1/config"]);
  releaseConfig();
  await boot;
  await history;
  expect(order).toEqual(["start /api/v1/config", "config done", "start /api/v1/history"]);
});

test("a failed bootstrap does not block later requests", async () => {
  globalThis.fetch = (async (input: RequestInfo | URL) =>
    String(input) === "/api/v1/config"
      ? new Response("{}", { status: 500 })
      : new Response(JSON.stringify({ entries: [] }), { status: 200 })) as unknown as typeof fetch;
  await expect(startBootstrap(getConfig)).rejects.toBeDefined();
  await expect(listHistory()).resolves.toEqual([]);
});
