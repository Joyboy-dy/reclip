import { afterEach, expect, test } from "bun:test";
import { ApiFailure } from "../src/api/client";
import { SESSION_EXPIRED, adminErrorText, isUnauthorized, listKeys, parseRetryAfter } from "../src/admin/api";
import { capitalize } from "../src/admin/lib";
import { parseRoute, routePath } from "../src/admin/route";

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

const stub = (status: number, body: unknown, headers: Record<string, string> = {}) => {
  globalThis.fetch = (async () => new Response(JSON.stringify(body), { status, headers })) as unknown as typeof fetch;
};

test("adminErrorText: 401 is a session expiry unless the caller overrides it", () => {
  const err = new ApiFailure("unauthorized", "no", 401);
  expect(adminErrorText(err)).toBe(SESSION_EXPIRED);
  expect(adminErrorText(err, "Wrong email or password.")).toBe("Wrong email or password.");
  expect(isUnauthorized(err)).toBe(true);
  expect(isUnauthorized(new ApiFailure("invalid_request", "bad", 400))).toBe(false);
});

test("adminErrorText: 429 uses Retry-After seconds, else a generic wait", () => {
  expect(adminErrorText(new ApiFailure("rate_limited", "slow", 429, 30))).toBe("Too many attempts. Try again in 30 s.");
  expect(adminErrorText(new ApiFailure("rate_limited", "slow", 429))).toBe("Too many attempts. Try again in a moment.");
});

test("adminErrorText: other server errors keep their message, network failures get fixed copy", () => {
  expect(adminErrorText(new ApiFailure("invalid_request", "Current password is not correct", 400))).toBe(
    "Current password is not correct",
  );
  expect(adminErrorText(new TypeError("fetch failed"))).toBe("Network error. Check your connection.");
});

test("parseRetryAfter accepts integer seconds only", () => {
  expect(parseRetryAfter("12")).toBe(12);
  expect(parseRetryAfter(" 7 ")).toBe(7);
  expect(parseRetryAfter("Wed, 21 Oct 2026 07:28:00 GMT")).toBeUndefined();
  expect(parseRetryAfter(null)).toBeUndefined();
});

test("requests surface Retry-After on ApiFailure", async () => {
  stub(429, { code: "rate_limited", message: "slow down" }, { "Retry-After": "42" });
  const err = await listKeys().catch((e) => e);
  expect(err).toBeInstanceOf(ApiFailure);
  expect(err.status).toBe(429);
  expect(err.retryAfter).toBe(42);
});

test("parseRoute / routePath", () => {
  expect(parseRoute("/admin")).toBe("home");
  expect(parseRoute("/admin/")).toBe("home");
  expect(parseRoute("/admin/keys")).toBe("keys");
  expect(parseRoute("/admin/keys/")).toBe("keys");
  expect(parseRoute("/admin/password")).toBe("password");
  expect(parseRoute("/admin/unknown")).toBe("home");
  expect(routePath("home")).toBe("/admin");
  expect(routePath("keys")).toBe("/admin/keys");
  expect(routePath("password")).toBe("/admin/password");
});

test("capitalize", () => {
  expect(capitalize("yesterday")).toBe("Yesterday");
  expect(capitalize("")).toBe("");
});
