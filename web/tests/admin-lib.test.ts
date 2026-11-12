import { expect, test } from "bun:test";
import type { ApiKeyInfo, Scope } from "../src/api/types.gen";
import {
  DEFAULT_NEW_KEY,
  buildCreateRequest,
  daysLeft,
  expiryInfo,
  keyStatus,
  limitsLabel,
  passwordStrength,
  relativeTime,
  rightsLabel,
  validateNewPassword,
} from "../src/admin/lib";

const NOW = new Date("2026-11-12T12:00:00Z");
const day = 86_400_000;
const iso = (ms: number) => new Date(NOW.getTime() + ms).toISOString();

test("daysLeft rounds partial days up and clamps at zero", () => {
  expect(daysLeft(iso(3 * day), NOW)).toBe(3);
  expect(daysLeft(iso(2.2 * day), NOW)).toBe(3);
  expect(daysLeft(iso(60_000), NOW)).toBe(1);
  expect(daysLeft(iso(-day), NOW)).toBe(0);
});

test("expiryInfo flags soon at 7 days or fewer, expired in the past", () => {
  expect(expiryInfo(null, NOW)).toBeNull();
  expect(expiryInfo(iso(82 * day), NOW)).toMatchObject({ status: "ok", daysLeft: 82, text: "82 days left" });
  expect(expiryInfo(iso(7 * day), NOW)?.status).toBe("soon");
  expect(expiryInfo(iso(8 * day), NOW)?.status).toBe("ok");
  expect(expiryInfo(iso(day), NOW)?.text).toBe("1 day left");
  expect(expiryInfo(iso(-1000), NOW)).toMatchObject({ status: "expired", text: "Expired" });
  expect(expiryInfo(iso(3 * day), NOW)?.date).toBe("Nov 15, 2026");
});

const base: ApiKeyInfo = {
  id: "k",
  name: "n",
  scopes: [],
  unlimited: false,
  limits: { probes_per_minute: null, jobs_per_hour: null, max_active: null },
  created_at: iso(-day),
  expires_at: null,
  revoked_at: null,
  last_used_at: null,
  request_count: 0,
};

test("keyStatus: revoked wins over expired over active", () => {
  expect(keyStatus(base, NOW)).toBe("active");
  expect(keyStatus({ ...base, expires_at: iso(-1000) }, NOW)).toBe("expired");
  expect(keyStatus({ ...base, expires_at: iso(-1000), revoked_at: iso(-5000) }, NOW)).toBe("revoked");
});

test("rightsLabel summarises scopes", () => {
  const four: Scope[] = ["probe", "jobs:create", "jobs:read", "jobs:cancel"];
  expect(rightsLabel([...four, "delegate"])).toBe("all + delegate");
  expect(rightsLabel(four)).toBe("all but delegate");
  expect(rightsLabel(["probe"])).toBe("probe");
  expect(rightsLabel(["probe", "jobs:create", "jobs:read", "jobs:cancel"].slice(1) as Scope[])).toBe("jobs");
  expect(rightsLabel(["jobs:read"])).toBe("jobs:read");
  expect(rightsLabel(["jobs:read"], true)).toBe("Read jobs");
  expect(rightsLabel([])).toBe("none");
});

test("limitsLabel", () => {
  expect(limitsLabel({ ...base, unlimited: true })).toBe("Unlimited");
  expect(limitsLabel(base)).toBe("default");
  expect(limitsLabel({ ...base, scopes: ["delegate"] })).toBe("per Telegram user");
  expect(limitsLabel({ ...base, limits: { probes_per_minute: null, jobs_per_hour: 200, max_active: 100 } })).toBe(
    "200/h · 100 active",
  );
  expect(limitsLabel({ ...base, limits: { probes_per_minute: 30, jobs_per_hour: null, max_active: null } }, true)).toBe(
    "30 probes/min",
  );
  expect(limitsLabel(base, true)).toBe("Default");
});

test("relativeTime", () => {
  expect(relativeTime(null, NOW)).toBe("never");
  expect(relativeTime(iso(-5000), NOW)).toBe("5 s ago");
  expect(relativeTime(iso(-2 * 60_000), NOW)).toBe("2 min ago");
  expect(relativeTime(iso(-3 * 3_600_000), NOW)).toBe("3 h ago");
  expect(relativeTime(iso(-day), NOW)).toBe("yesterday");
  expect(relativeTime(iso(-3 * day), NOW)).toBe("3 days ago");
  expect(relativeTime(iso(-21 * day), NOW)).toBe("3 weeks ago");
  expect(relativeTime(iso(-90 * day), NOW)).toBe("Aug 14, 2026");
});

test("passwordStrength grows with length and variety", () => {
  expect(passwordStrength("").bars).toBe(0);
  expect(passwordStrength("short1").bars).toBe(1);
  expect(passwordStrength("aaaaaaaaaaaa").bars).toBe(2);
  expect(passwordStrength("Abcdefgh1234").bars).toBe(3);
  expect(passwordStrength("Abcdefgh1234!xyz").bars).toBe(4);
  expect(passwordStrength("Abcdefgh1234").label).toBe("Strong. At least 12 characters.");
  expect(passwordStrength("short").label).toBe("Too short. At least 12 characters.");
});

test("buildCreateRequest", () => {
  const ok = { ...DEFAULT_NEW_KEY, name: " Bot " };
  expect(buildCreateRequest(ok)).toEqual({
    request: {
      name: "Bot",
      scopes: DEFAULT_NEW_KEY.scopes,
      unlimited: false,
      expires_in_days: 90,
      limits: { probes_per_minute: null, jobs_per_hour: null, max_active: null },
    },
  });
  const never = buildCreateRequest({ ...ok, expires: "never", unlimited: true, jobs: "abc" });
  expect(never).toEqual({ request: { name: "Bot", scopes: DEFAULT_NEW_KEY.scopes, unlimited: true } });
  const limited = buildCreateRequest({ ...ok, probes: "30", jobs: "200", active: "5" });
  expect("request" in limited && limited.request.limits).toEqual({
    probes_per_minute: 30,
    jobs_per_hour: 200,
    max_active: 5,
  });
  expect(buildCreateRequest({ ...ok, name: "  " })).toEqual({ error: "Give the key a name." });
  expect(buildCreateRequest({ ...ok, scopes: [] })).toEqual({ error: "Pick at least one right." });
  expect(buildCreateRequest({ ...ok, jobs: "0" })).toEqual({ error: "Limits must be whole numbers above 0." });
  expect(buildCreateRequest({ ...ok, active: "1.5" })).toEqual({ error: "Limits must be whole numbers above 0." });
});

test("validateNewPassword", () => {
  expect(validateNewPassword("a".repeat(11), "a".repeat(11))).toBe("Use at least 12 characters.");
  expect(validateNewPassword("a".repeat(12), "b".repeat(12))).toBe("The two passwords do not match.");
  expect(validateNewPassword("a".repeat(12), "a".repeat(12), "a".repeat(12))).toBe(
    "Choose a password different from the current one.",
  );
  expect(validateNewPassword("a".repeat(12), "a".repeat(12), "other")).toBeNull();
  expect(validateNewPassword("a".repeat(12), "a".repeat(12))).toBeNull();
});
