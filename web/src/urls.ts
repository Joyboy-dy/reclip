/** Split on whitespace/commas, keep http(s) links only, dedupe preserving order. */
export function parseUrls(text: string): string[] {
  const parts = text.split(/[\s,]+/).filter((u) => u.startsWith("http://") || u.startsWith("https://"));
  return [...new Set(parts)];
}

export function fmtDur(s: number | null | undefined): string {
  if (!s) return "";
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${sec.toString().padStart(2, "0")}`;
}

export function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
