import type { ApiKeyInfo, CreateKeyRequest, Scope } from "@/api/types.gen";

const DAY_MS = 86_400_000;
export const MIN_PASSWORD = 12;
export const SOON_DAYS = 7;

const dateFmt = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" });
export const formatDate = (iso: string): string => dateFmt.format(new Date(iso));

/** Whole days until `iso`, rounded up so "23 hours left" reads as 1 day; never negative. */
export function daysLeft(iso: string, now: Date = new Date()): number {
  return Math.max(0, Math.ceil((new Date(iso).getTime() - now.getTime()) / DAY_MS));
}

export type ExpiryStatus = "ok" | "soon" | "expired";
export type ExpiryInfo = { status: ExpiryStatus; daysLeft: number; date: string; text: string };

/** Expiry details for the calendar icon / card line; null when the key never expires. */
export function expiryInfo(expiresAt: string | null, now: Date = new Date()): ExpiryInfo | null {
  if (!expiresAt) return null;
  const date = formatDate(expiresAt);
  if (new Date(expiresAt).getTime() <= now.getTime()) return { status: "expired", daysLeft: 0, date, text: "Expired" };
  const days = daysLeft(expiresAt, now);
  return {
    status: days <= SOON_DAYS ? "soon" : "ok",
    daysLeft: days,
    date,
    text: `${days} ${days === 1 ? "day" : "days"} left`,
  };
}

export type KeyStatus = "active" | "expired" | "revoked";
export function keyStatus(key: ApiKeyInfo, now: Date = new Date()): KeyStatus {
  if (key.revoked_at) return "revoked";
  return expiryInfo(key.expires_at, now)?.status === "expired" ? "expired" : "active";
}

const JOB_SCOPES: Scope[] = ["jobs:create", "jobs:read", "jobs:cancel"];
const FRIENDLY: Record<string, string> = {
  "jobs:create": "start jobs",
  "jobs:read": "read jobs",
  "jobs:cancel": "cancel jobs",
};
export const capitalize = (s: string): string => s.charAt(0).toUpperCase() + s.slice(1);

/** Short rights summary. Table uses raw scope names; cards (`friendly`) use readable ones, capitalised. */
export function rightsLabel(scopes: Scope[], friendly = false): string {
  const has = (s: Scope) => scopes.includes(s);
  const allJobs = JOB_SCOPES.every(has);
  let text: string;
  if (has("probe") && allJobs) {
    text = has("delegate") ? "all + delegate" : "all but delegate";
  } else {
    const parts: string[] = [];
    if (has("probe")) parts.push("probe");
    if (allJobs) parts.push("jobs");
    else for (const s of JOB_SCOPES) if (has(s)) parts.push(friendly ? FRIENDLY[s]! : s);
    if (has("delegate")) parts.push("delegate");
    text = parts.join(", ") || "none";
  }
  return friendly ? capitalize(text) : text;
}

export function limitsLabel(key: ApiKeyInfo, friendly = false): string {
  const { probes_per_minute: p, jobs_per_hour: j, max_active: a } = key.limits;
  const parts: string[] = [];
  if (p !== null) parts.push(`${p} probes/min`);
  if (j !== null) parts.push(`${j}/h`);
  if (a !== null) parts.push(`${a} active`);
  let text = "default";
  if (key.unlimited) text = "Unlimited";
  else if (parts.length) text = parts.join(" · ");
  else if (key.scopes.includes("delegate")) text = "per Telegram user";
  return friendly ? capitalize(text) : text;
}

export function relativeTime(iso: string | null, now: Date = new Date()): string {
  if (!iso) return "never";
  const secs = Math.max(0, Math.floor((now.getTime() - new Date(iso).getTime()) / 1000));
  if (secs < 60) return `${secs} s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)} min ago`;
  if (secs < 86_400) return `${Math.floor(secs / 3600)} h ago`;
  const days = Math.floor(secs / 86_400);
  if (days === 1) return "yesterday";
  if (days < 14) return `${days} days ago`;
  if (days < 60) return `${Math.floor(days / 7)} weeks ago`;
  return formatDate(iso);
}

export const keyPrefix = (id: string): string => `rck_${id}_…`;

export function passwordStrength(pw: string): { bars: 0 | 1 | 2 | 3 | 4; label: string } {
  if (!pw) return { bars: 0, label: `At least ${MIN_PASSWORD} characters.` };
  if (pw.length < MIN_PASSWORD) return { bars: 1, label: `Too short. At least ${MIN_PASSWORD} characters.` };
  const classes = [/[a-z]/, /[A-Z]/, /\d/, /[^A-Za-z0-9]/].filter((re) => re.test(pw)).length;
  const varied = classes >= 3;
  const bars = (2 + (varied ? 1 : 0) + (varied && pw.length >= 16 ? 1 : 0)) as 2 | 3 | 4;
  return {
    bars,
    label: varied
      ? `Strong. At least ${MIN_PASSWORD} characters.`
      : "OK. Mixing letters, digits and symbols makes it stronger.",
  };
}

export function validateNewPassword(next: string, confirm: string, current?: string): string | null {
  if (next.length < MIN_PASSWORD) return `Use at least ${MIN_PASSWORD} characters.`;
  if (next !== confirm) return "The two passwords do not match.";
  if (current !== undefined && next === current) return "Choose a password different from the current one.";
  return null;
}

export const EXPIRY_CHOICES = [
  { value: "7", label: "In 7 days" },
  { value: "30", label: "In 30 days" },
  { value: "90", label: "In 90 days" },
  { value: "365", label: "In 1 year" },
  { value: "never", label: "Never" },
] as const;

export const SCOPE_CHOICES: { scope: Scope; label: string; phoneLabel?: string }[] = [
  { scope: "probe", label: "Analyse links" },
  { scope: "jobs:create", label: "Start downloads" },
  { scope: "jobs:read", label: "Read downloads and files" },
  { scope: "jobs:cancel", label: "Cancel and remove" },
  { scope: "delegate", label: "Act for Telegram users (only for the bot)", phoneLabel: "Act for Telegram users" },
];

export type NewKeyForm = {
  name: string;
  scopes: Scope[];
  expires: string;
  unlimited: boolean;
  probes: string;
  jobs: string;
  active: string;
};

export const DEFAULT_NEW_KEY: NewKeyForm = {
  name: "",
  scopes: ["probe", "jobs:create", "jobs:read", "jobs:cancel"],
  expires: "90",
  unlimited: false,
  probes: "",
  jobs: "",
  active: "",
};

function parseLimit(raw: string): number | null | undefined {
  const text = raw.trim();
  if (!text) return null;
  return /^\d+$/.test(text) && Number(text) > 0 ? Number(text) : undefined;
}

/** Turns the form into an API request, or a user-facing error. Empty limits stay null (server default). */
export function buildCreateRequest(f: NewKeyForm): { request: CreateKeyRequest } | { error: string } {
  const name = f.name.trim();
  if (!name) return { error: "Give the key a name." };
  if (f.scopes.length === 0) return { error: "Pick at least one right." };
  const probes = parseLimit(f.probes);
  const jobs = parseLimit(f.jobs);
  const active = parseLimit(f.active);
  if (!f.unlimited && (probes === undefined || jobs === undefined || active === undefined)) {
    return { error: "Limits must be whole numbers above 0." };
  }
  const request: CreateKeyRequest = { name, scopes: f.scopes, unlimited: f.unlimited };
  if (f.expires !== "never") request.expires_in_days = Number(f.expires);
  if (!f.unlimited) {
    request.limits = { probes_per_minute: probes ?? null, jobs_per_hour: jobs ?? null, max_active: active ?? null };
  }
  return { request };
}
