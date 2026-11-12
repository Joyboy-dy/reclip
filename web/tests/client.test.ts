import { expect, test } from "bun:test";
import { probe } from "../src/api/client";

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
