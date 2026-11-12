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

test("no source file or page generates an inline style attribute", () => {
  const files = [join(import.meta.dir, "../index.html")];
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.isDirectory()) walk(join(dir, e.name));
      else if (e.name.endsWith(".ts")) files.push(join(dir, e.name));
    }
  };
  walk(join(import.meta.dir, "../src"));
  for (const f of files) expect([f, /\bstyle\s*=/.test(readFileSync(f, "utf8"))]).toEqual([f, false]);
});
