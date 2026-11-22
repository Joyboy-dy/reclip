import { expect, test } from "bun:test";
import type { ClientInfo } from "../src/api/types.gen";
import { blockTarget, bytes, clientName, clientSub, duration, filterClients, share } from "../src/admin/activity-lib";

const client = (over: Partial<ClientInfo>): ClientInfo => ({
  owner: "web:0123456789abcdef0123456789abcdef",
  kind: "web",
  label: "Chrome 131 · Windows · desktop",
  device: "Chrome 131 · Windows · desktop",
  tg_username: null,
  tg_name: null,
  first_seen: "2026-11-20T10:00:00.000Z",
  last_seen: "2026-11-22T10:00:00.000Z",
  last_ip: "203.0.113.7",
  ips_day: 1,
  ips_total: 1,
  ips: [{ ip: "203.0.113.7", last_seen: "2026-11-22T10:00:00.000Z", hits: 3 }],
  downloads_day: 1,
  downloads_week: 2,
  downloads_total: 3,
  bytes_total: 0,
  block: null,
  flags: [],
  ...over,
});

test("sizes and durations", () => {
  expect([bytes(0), bytes(2048), bytes(5 * 1024 * 1024), bytes(3.4 * 1024 ** 3)]).toEqual(["0 B", "2 KB", "5.0 MB", "3.4 GB"]);
  expect([duration(42), duration(125), duration(3 * 3600 + 720), duration(50 * 3600)]).toEqual(["42 s", "2 min", "3 h 12 min", "2 days 2 h"]);
  expect([share(50, 200), share(null, 1), share(1, null), share(500, 100)]).toEqual([25, null, null, 100]);
});

test("labels", () => {
  expect(clientSub(client({}))).toBe("Web · desktop · 01234567…");
  expect(clientName(client({}))).toBe("Chrome 131 on Windows");
  const tg = client({ owner: "tg:42", kind: "telegram", label: "@jean", tg_username: "jean", tg_name: "Jean D", device: null });
  expect(clientSub(tg)).toBe("Telegram · 42 · Jean D");
  const block = { id: 1, kind: "ip" as const, target: "203.0.113.7/32", reason: "", created_at: "", created_by: null };
  expect(blockTarget(block)).toBe("203.0.113.7");
  expect(blockTarget({ ...block, kind: "owner", target: "tg:42" })).toBe("Telegram 42");
});

test("filters and search", () => {
  const list = [
    client({}),
    client({ owner: "tg:42", kind: "telegram", label: "@jean", flags: ["many_ips"] }),
    client({ owner: "key:ab", kind: "key", label: "agent", block: { id: 1, kind: "owner", target: "key:ab", reason: "", created_at: "", created_by: null } }),
  ];
  expect(filterClients(list, "telegram", "").map((c) => c.owner)).toEqual(["tg:42"]);
  expect(filterClients(list, "flagged", "").length).toBe(1);
  expect(filterClients(list, "blocked", "").map((c) => c.owner)).toEqual(["key:ab"]);
  expect(filterClients(list, "all", "203.0.113").length).toBe(3);
  expect(filterClients(list, "all", "JEAN").map((c) => c.owner)).toEqual(["tg:42"]);
});
