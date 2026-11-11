import { cp, mkdir, readdir, rm } from "node:fs/promises";
import { watch as fsWatch } from "node:fs";

const watch = process.argv.includes("--watch");

async function build(): Promise<void> {
  await rm("dist", { recursive: true, force: true });
  await mkdir("dist", { recursive: true });

  const result = await Bun.build({
    entrypoints: ["src/main.ts"],
    outdir: "dist",
    target: "browser",
    minify: true,
    naming: "app.js",
  });
  if (!result.success) {
    for (const log of result.logs) console.error(log);
    throw new Error("bundle failed");
  }

  const css = await Promise.all(["base", "form", "cards"].map((n) => Bun.file(`styles/${n}.css`).text()));
  await Bun.write("dist/styles.css", css.join("\n"));

  await cp("index.html", "dist/index.html");
  for (const f of await readdir("static")) await cp(`static/${f}`, `dist/${f}`);
  console.log("built dist/");
}

await build();

if (watch) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  for (const path of ["src", "styles", "static", "index.html"]) {
    fsWatch(path, { recursive: true }, () => {
      clearTimeout(timer);
      timer = setTimeout(() => build().catch((e) => console.error(e)), 100);
    });
  }
  console.log("watching for changes...");
}
