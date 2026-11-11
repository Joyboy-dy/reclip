import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { defineConfig, devices } from "@playwright/test";

// Specs are named *.e2e.ts so that `bun test` (which matches *.spec.ts) ignores them.
const exe = process.platform === "win32" ? ".exe" : "";
const targetDir = path.resolve(import.meta.dirname, "../../target/debug");
const webDir = path.resolve(import.meta.dirname, "../dist");
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "reclip-e2e-"));
const port = 18999;

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.e2e.ts",
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: [["list"], ["html", { open: "never", outputFolder: "../playwright-report" }]],
  outputDir: "../test-results",
  use: { baseURL: `http://127.0.0.1:${port}`, trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: path.join(targetDir, `reclip-core${exe}`),
    env: {
      RECLIP_YTDLP_BIN: path.join(targetDir, `fake-ytdlp${exe}`),
      RECLIP_PORT: String(port),
      RECLIP_HOST: "127.0.0.1",
      RECLIP_DATA_DIR: dataDir,
      RECLIP_WEB_DIR: webDir,
      RECLIP_MAX_CONCURRENT: "1",
      RECLIP_NO_UPDATE: "1",
    },
    url: `http://127.0.0.1:${port}/healthz`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
