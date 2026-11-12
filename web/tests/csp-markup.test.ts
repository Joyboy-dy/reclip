import { expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { progressBarHtml } from "../src/ui/progress-bar";

test("progress_markup_has_no_style_attribute", () => {
  for (const state of ["queued", "downloading", "processing"] as const) {
    const html = progressBarHtml({ progress: 42, speed: 1e6, eta: 5, state });
    expect(html).not.toContain("style=");
  }
});

test("no source file or page has an inline style attribute or a literal style tag", () => {
  const files = ["index.html", "admin.html"].map((f) => join(import.meta.dir, "..", f));
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.isDirectory()) walk(join(dir, e.name));
      else if (/\.tsx?$/.test(e.name)) files.push(join(dir, e.name));
    }
  };
  walk(join(import.meta.dir, "../src"));
  for (const f of files) {
    const text = readFileSync(f, "utf8");
    expect([f, /\bstyle\s*=/.test(text)]).toEqual([f, false]);
    expect([f, /<style/i.test(text)]).toEqual([f, false]);
  }
});

test("select viewport receives the CSP nonce", () => {
  const src = readFileSync(join(import.meta.dir, "../src/components/ui/select.tsx"), "utf8");
  expect(src).toContain("<SelectPrimitive.Viewport nonce={styleNonce()}");
});
