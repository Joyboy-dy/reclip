import type { BlockInfo, ClientFlag, ClientInfo, ClientKind } from "@/api/types.gen";

const KIB = 1024;
const MIB = KIB * 1024;
const GIB = MIB * 1024;

/** "0 B", "740 KB", "12 MB", "3.4 GB". */
export function bytes(n: number): string {
  if (n >= GIB) return `${(n / GIB).toFixed(1)} GB`;
  if (n >= 10 * MIB) return `${Math.round(n / MIB)} MB`;
  if (n >= MIB) return `${(n / MIB).toFixed(1)} MB`;
  if (n >= KIB) return `${Math.round(n / KIB)} KB`;
  return `${n} B`;
}

/** "3 h 12 min", "42 s", "2 days 4 h". */
export function duration(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  if (h < 48) return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
  const d = Math.floor(h / 24);
  return h % 24 ? `${d} days ${h % 24} h` : `${d} days`;
}

export const KIND_LABELS: Record<ClientKind, string> = { web: "Web", telegram: "Telegram", key: "API key" };

export const FLAG_LABELS: Record<ClientFlag, string> = {
  many_ips: "Many addresses",
  shared_ip: "Shared address",
};

export const FLAG_HINTS: Record<ClientFlag, string> = {
  many_ips: "5 or more addresses in 24 h: the link may be used by several people.",
  shared_ip: "Its last address was used by 4 or more browsers this week.",
};

/** The id part of an owner, shortened: "3f9a1c2b…", "123456789". */
export function shortId(owner: string): string {
  const id = owner.slice(owner.indexOf(":") + 1);
  return id.length > 12 ? `${id.slice(0, 8)}…` : id;
}

/** A browser's name: "Chrome 131 on Windows" (from "Chrome 131 · Windows · desktop"). */
export function clientName(c: ClientInfo): string {
  if (c.kind !== "web" || !c.device) return c.label;
  const [browser, system] = c.device.split(" · ");
  return system ? `${browser} on ${system}` : (browser ?? c.label);
}

/** Second line under a client's name: "Web · mobile · 3f9a1c2b…", "Telegram · 42 · José D". */
export function clientSub(c: ClientInfo): string {
  const parts = [KIND_LABELS[c.kind]];
  const form = c.kind === "web" ? c.device?.split(" · ")[2] : undefined;
  if (form) parts.push(form);
  parts.push(shortId(c.owner));
  if (c.kind === "telegram" && c.tg_username && c.tg_name) parts.push(c.tg_name);
  if (c.kind !== "web" && c.device) parts.push(c.device);
  return parts.join(" · ");
}

export type Filter = "all" | "web" | "telegram" | "key" | "flagged" | "blocked";

export const FILTERS: { value: Filter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "web", label: "Web" },
  { value: "telegram", label: "Telegram" },
  { value: "key", label: "Keys" },
  { value: "flagged", label: "Flagged" },
  { value: "blocked", label: "Blocked" },
];

/** Clients that pass `filter` and contain `query` (name, device, owner or an address). */
export function filterClients(clients: ClientInfo[], filter: Filter, query: string): ClientInfo[] {
  const q = query.trim().toLowerCase();
  return clients.filter((c) => {
    const kept =
      filter === "all" ||
      (filter === "flagged" && c.flags.length > 0) ||
      (filter === "blocked" && c.block !== null) ||
      c.kind === filter;
    if (!kept || !q) return kept;
    const hay = [c.label, c.device, c.owner, c.tg_name, c.tg_username, c.last_ip, ...c.ips.map((i) => i.ip)];
    return hay.some((h) => h?.toLowerCase().includes(q));
  });
}

/** What a block is about: "Device 3f9a1c2b…", "Telegram 42", "Key ab12cd34", "203.0.113.0/24". */
export function blockTarget(b: BlockInfo): string {
  if (b.kind === "ip") return b.target.replace(/\/(32|128)$/, "");
  const [kind, id] = [b.target.slice(0, b.target.indexOf(":")), shortId(b.target)];
  return `${kind === "tg" ? "Telegram" : kind === "key" ? "Key" : "Device"} ${id}`;
}

/** "62%" of a limit, or null when there is none. */
export const share = (used: number | null, limit: number | null): number | null =>
  used === null || !limit ? null : Math.min(100, (used / limit) * 100);
