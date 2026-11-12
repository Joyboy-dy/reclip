/** Split on whitespace/commas, keep http(s) links only, dedupe preserving order. */
export function parseUrls(text: string): string[] {
  const parts = text.split(/[\s,]+/).filter((u) => u.startsWith("http://") || u.startsWith("https://"));
  return [...new Set(parts)];
}
