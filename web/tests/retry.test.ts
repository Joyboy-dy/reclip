import { expect, test } from "bun:test";
import { ApiFailure } from "../src/api/client";
import { MAX_RATE_RETRIES, rateLimitWait } from "../src/retry";

const limited = (retryAfter?: number) => new ApiFailure("rate_limited", "Too many", 429, retryAfter);

test("a rate limit with a short Retry-After waits that long", () => {
  expect(rateLimitWait(limited(30), 0)).toBe(30);
  expect(rateLimitWait(limited(120), 0)).toBe(120);
  expect(rateLimitWait(limited(0), 0)).toBe(0);
});

test("long, missing waits and other errors give up", () => {
  expect(rateLimitWait(limited(121), 0)).toBeUndefined();
  expect(rateLimitWait(limited(undefined), 0)).toBeUndefined();
  expect(rateLimitWait(new ApiFailure("forbidden", "no", 403, 30), 0)).toBeUndefined();
  expect(rateLimitWait(new Error("network"), 0)).toBeUndefined();
});

test("at most three retries", () => {
  expect(MAX_RATE_RETRIES).toBe(3);
  expect(rateLimitWait(limited(5), 2)).toBe(5);
  expect(rateLimitWait(limited(5), 3)).toBeUndefined();
});
