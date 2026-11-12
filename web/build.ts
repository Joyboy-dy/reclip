import { cp, mkdir, readdir, rm } from "node:fs/promises";
import { watch as fsWatch } from "node:fs";

const watch = process.argv.includes("--watch");

async function bundle(entry: string, name: string): Promise<void> {
  const result = await Bun.build({
    entrypoints: [entry],
    outdir: "dist",
    target: "browser",
    minify: true,
    naming: name,
    define: { "process.env.NODE_ENV": '"production"' },
  });
  if (!result.success) {
    for (const log of result.logs) console.error(log);
    throw new Error(`bundle failed: ${entry}`);
  }
}

async function tailwind(): Promise<void> {
  const proc = Bun.spawn(
    [process.execPath, "x", "tailwindcss", "-i", "src/styles/app.css", "-o", "dist/app.css", "--minify"],
    { stdout: "inherit", stderr: "inherit" },
  );
  if ((await proc.exited) !== 0) throw new Error("tailwind failed");
}

async function build(): Promise<void> {
  await rm("dist", { recursive: true, force: true });
  await mkdir("dist", { recursive: true });

  // Public page: still the framework-free bundle until it moves to React.
  await bundle("src/main.ts", "app.js");
  await bundle("src/admin/main.tsx", "admin.js");
  await tailwind();

  const css = await Promise.all(["base", "form", "cards"].map((n) => Bun.file(`styles/${n}.css`).text()));
  await Bun.write("dist/styles.css", css.join("\n"));

  await cp("index.html", "dist/index.html");
  await cp("admin.html", "dist/admin.html");
  for (const f of await readdir("static")) await cp(`static/${f}`, `dist/${f}`);
  console.log("built dist/");
}

await build();

if (watch) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  for (const path of ["src", "styles", "static", "index.html", "admin.html"]) {
    fsWatch(path, { recursive: true }, () => {
      clearTimeout(timer);
      timer = setTimeout(() => build().catch((e) => console.error(e)), 100);
    });
  }
  console.log("watching for changes...");
}
