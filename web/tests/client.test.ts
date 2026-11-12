import { expect, test } from "bun:test";
import { probe } from "../src/api/client";

test("client throws ApiFailure with the server code", async () => {
  globalThis.fetch = (async () =>
    new Response(JSON.stringify({ code: "bot_check", message: "m" }), { status: 422 })) as unknown as typeof fetch;
  await expect(probe("https://x")).rejects.toMatchObject({ code: "bot_check", status: 422 });
});
