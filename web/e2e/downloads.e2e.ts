import type { Page } from "@playwright/test";
import { apiJobs, cards, expect, fetchUrl, test } from "./fixtures";

const fillWidth = (page: Page) =>
  cards(page).first().locator("[data-progress-fill]").evaluate((el) => el.getBoundingClientRect().width);

test("paste, fetch and download mp3", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("radio", { name: "Audio" }).click();
  await page.getByRole("radio", { name: "MP3" }).click();
  await page.locator("#urls").fill("https://93.184.216.34/ok");
  await page.getByRole("button", { name: "Fetch" }).click();
  const card = cards(page).first();
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await expect(card.getByRole("link", { name: "Save" })).toBeVisible();
  await expect(card).toContainText(".mp3");
});

test("clip, metadata and container are sent with the job", async ({ page }) => {
  const card = await fetchUrl(page, "https://93.184.216.34/ok");
  await page.getByRole("radio", { name: "MKV" }).click();
  await page.getByRole("checkbox", { name: "Clip" }).click();
  await page.getByLabel("from").fill("1:20");
  await page.getByLabel("to", { exact: true }).fill("2:05");
  await page.getByRole("checkbox", { name: "Remove metadata" }).click();
  const create = page.waitForRequest((r) => r.url().endsWith("/api/v1/jobs") && r.method() === "POST");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  expect((await create).postDataJSON()).toMatchObject({ format: "mkv", clip: { start: 80, end: 125 }, strip_metadata: true });
  await expect(card.getByRole("link", { name: "Save" })).toBeVisible();
  await expect(card).toContainText("metadata removed", { ignoreCase: true });
});

test("progress survives a refresh", async ({ page }) => {
  const card = await fetchUrl(page, "https://93.184.216.34/slow");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await expect.poll(async () => (await apiJobs(page)).some((j) => (j.progress ?? 0) > 0)).toBe(true);
  // Computed width (what the user sees), not just the inline value.
  await expect.poll(() => fillWidth(page)).toBeGreaterThan(0);
  await page.reload();
  const restored = cards(page).first();
  await expect(restored.getByRole("progressbar")).toBeVisible();
  await expect.poll(() => fillWidth(page)).toBeGreaterThan(0);
  await expect(restored.getByRole("button", { name: "Cancel" })).toBeVisible();
});

test("cancel a running download", async ({ page }) => {
  const card = await fetchUrl(page, "https://93.184.216.34/slow");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await card.getByRole("button", { name: "Cancel" }).click();
  await expect(card).toContainText(/cancel/i);
  await expect(card.getByRole("button", { name: "Remove" })).toBeVisible();
});

test("remove a finished download", async ({ page }) => {
  const card = await fetchUrl(page, "https://93.184.216.34/ok");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await expect(card.getByRole("link", { name: "Save" })).toBeVisible();
  await card.getByRole("button", { name: "Remove" }).click();
  await expect(cards(page)).toHaveCount(0);
  await page.reload();
  await expect(page.locator("#urls")).toBeVisible();
  await expect(cards(page)).toHaveCount(0);
});

test("clean up menu opens with its four actions", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Clean up" }).click();
  const menu = page.getByRole("menu");
  for (const item of ["Remove expired files from the list", "Remove older than 7 days", "Remove older than 14 days", "Clear all history"]) {
    await expect(menu.getByRole("menuitem", { name: item })).toBeVisible();
  }
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});

// Needs GET /api/v1/history (Task B2; 501 until then). The controller re-enables it after merge.
test.fixme("a finished download is listed in the history with Save, and can be removed", async ({ page }) => {
  const card = await fetchUrl(page, "https://93.184.216.34/ok");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await expect(card.getByRole("link", { name: "Save" })).toBeVisible();
  const row = page.locator("[data-history-row]").first();
  await expect(row.getByRole("link", { name: "Save" })).toBeVisible();
  await row.getByRole("button", { name: "Remove from history" }).click();
  await expect(page.locator("[data-history-row]")).toHaveCount(0);
});

// Needs POST /api/v1/jobs/{id}/resume and resumable jobs (Task B2; 501 until then).
test.fixme("a cancelled download resumes", async ({ page }) => {
  const card = await fetchUrl(page, "https://93.184.216.34/slow");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await expect.poll(() => fillWidth(page)).toBeGreaterThan(0);
  await card.getByRole("button", { name: "Cancel" }).click();
  await expect(card).toContainText("Resume keeps the part already downloaded");
  await card.getByRole("button", { name: "Resume" }).click();
  await expect(card.getByRole("button", { name: "Cancel" })).toBeVisible();
});
