import { expect, test } from "bun:test";
import { clipFromInput, clipText, fmtClock, fmtSize, fmtTtl, parseClock, relativeDay, siteName } from "../src/public/format";

test("parseClock reads seconds, m:ss and h:mm:ss", () => {
  expect(parseClock("80")).toBe(80);
  expect(parseClock(" 1:20 ")).toBe(80);
  expect(parseClock("1:02:03")).toBe(3723);
  expect(parseClock("")).toBeUndefined();
  expect(parseClock("1:75")).toBeNaN();
  expect(parseClock("abc")).toBeNaN();
  expect(parseClock("1:2:3:4")).toBeNaN();
});

test("clipFromInput: off, open end, bounded, and invalid times", () => {
  expect(clipFromInput({ on: false, from: "zz", to: "" })).toEqual({});
  expect(clipFromInput({ on: true, from: "1:20", to: "" })).toEqual({ clip: { start: 80 } });
  expect(clipFromInput({ on: true, from: "", to: "2:05" })).toEqual({ clip: { start: 0, end: 125 } });
  expect(clipFromInput({ on: true, from: "1:20", to: "2:05" })).toEqual({ clip: { start: 80, end: 125 } });
  expect(clipFromInput({ on: true, from: "2:05", to: "1:20" }).error).toBe("Clip end must come after its start");
  expect(clipFromInput({ on: true, from: "x", to: "" }).error).toBe("Clip times look like 1:20 or 80");
});

test("clock, size, ttl and clip texts", () => {
  expect(fmtClock(634)).toBe("10:34");
  expect(fmtClock(3725)).toBe("1:02:05");
  expect(fmtSize(18 * 1024 * 1024)).toBe("18 MB");
  expect(fmtSize(3.1 * 1024 * 1024)).toBe("3.1 MB");
  expect(fmtSize(840 * 1024)).toBe("840 KB");
  expect(fmtSize(1.25 * 1024 ** 3)).toBe("1.3 GB");
  expect(fmtTtl(21600)).toBe("6 h");
  expect(fmtTtl(2700)).toBe("45 min");
  expect(fmtTtl(172800)).toBe("2 days");
  expect(clipText({ start: 5, end: 30 })).toBe("clip 0:05 to 0:30");
  expect(clipText({ start: 80 })).toBe("clip from 1:20");
});

test("relativeDay counts calendar days", () => {
  const now = new Date(2026, 10, 12, 9, 0);
  expect(relativeDay(new Date(2026, 10, 12, 1, 0).toISOString(), now)).toBe("today");
  expect(relativeDay(new Date(2026, 10, 11, 23, 0).toISOString(), now)).toBe("yesterday");
  expect(relativeDay(new Date(2026, 10, 9, 12, 0).toISOString(), now)).toBe("3 days ago");
});

test("siteName names known sites and falls back to the host", () => {
  expect(siteName("https://www.youtube.com/watch?v=a")).toBe("YouTube");
  expect(siteName("https://youtu.be/a")).toBe("YouTube");
  expect(siteName("https://vm.tiktok.com/x")).toBe("TikTok");
  expect(siteName("https://x.com/a/status/1")).toBe("X");
  expect(siteName("https://www.example.org/v")).toBe("example.org");
  expect(siteName("not a url")).toBe("");
});
