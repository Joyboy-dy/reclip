import { expect, test } from "bun:test";
import type { Job } from "../src/api/types.gen";
import { friendlyError, jobError } from "../src/errors";

test("mapped code gets the friendly text", () => {
  expect(friendlyError("geo_blocked", "raw")).toBe("Video not available in your region");
});

test("bot_check keeps the server message", () => {
  expect(friendlyError("bot_check", "Add cookies to continue")).toBe("Add cookies to continue");
});

test("unknown long message is truncated to 80 chars", () => {
  expect(friendlyError("internal", "x".repeat(100))).toBe("x".repeat(80) + "...");
  expect(friendlyError(undefined, "short")).toBe("short");
});

test("cancelled job without error maps through the state", () => {
  const job = { state: "cancelled", error: null } as unknown as Job;
  expect(jobError(job)).toBe("Download cancelled");
  expect(jobError({ ...job, state: "expired" })).toBe("File expired — download it again");
});

test("internal 403 and copyright errors get friendly texts", () => {
  expect(friendlyError("internal", "ERROR: HTTP Error 403: Forbidden")).toBe(
    "Access denied by the platform",
  );
  expect(friendlyError("internal", "Blocked: Copyright claim by X")).toBe(
    "Video blocked due to copyright",
  );
});

test("too_long text mentions live streams", () => {
  expect(friendlyError("too_long", "raw")).toBe("Video is too long or is a live stream");
});

test("rate_limited keeps the server message", () => {
  expect(friendlyError("rate_limited", "Try again in 12 s")).toBe("Try again in 12 s");
});

test("access error codes have friendly texts", () => {
  expect(friendlyError("unauthorized", "raw")).toBe("This server requires an API key");
  expect(friendlyError("forbidden", "raw")).toBe("This action was refused");
  expect(friendlyError("forbidden_target", "raw")).toBe("This link points to a private address");
});
