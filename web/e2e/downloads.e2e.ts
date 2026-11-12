import { type Page, expect, test } from "@playwright/test";

type ApiJob = { id: string; state: string; progress: number | null };
const TERMINAL = ["done", "failed", "cancelled", "expired"];

const apiJobs = async (page: Page): Promise<ApiJob[]> =>
  ((await (await page.request.get("/api/v1/jobs")).json()) as { jobs: ApiJob[] }).jobs;

// RECLIP_MAX_CONCURRENT=1: whatever a spec did, free the only download slot
// (through the API, with the context's cookie) before the next spec starts.
test.afterEach(async ({ page }) => {
  for (const job of await apiJobs(page)) {
    if (!TERMINAL.includes(job.state)) await page.request.post(`/api/v1/jobs/${job.id}/cancel`, { data: {} });
  }
  await expect
    .poll(async () => (await apiJobs(page)).every((j) => TERMINAL.includes(j.state)))
    .toBe(true);
});

const barWidth = (page: Page) =>
  page.locator("#cards .card .pbar-fill").first().evaluate((el) => parseFloat((el as HTMLElement).style.width) || 0);

async function fetchUrl(page: Page, url: string) {
  await page.goto("/");
  await page.locator("#urls").fill(url);
  await page.locator("#goBtn").click();
  const card = page.locator("#cards .card").first();
  await expect(card.getByRole("button", { name: "Download", exact: true })).toBeVisible();
  return card;
}

test("paste, fetch and download mp3", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "MP3" }).click();
  await page.locator("#urls").fill("https://ok.example/1");
  await page.locator("#goBtn").click();
  const card = page.locator("#cards .card").first();
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await expect(card.getByRole("button", { name: "Save" })).toBeVisible();
  await expect(card.locator(".card-status.done")).toContainText(".mp3");
});

test("progress survives a refresh", async ({ page }) => {
  const card = await fetchUrl(page, "https://slow.example/1");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await expect.poll(async () => (await apiJobs(page)).some((j) => (j.progress ?? 0) > 0)).toBe(true);
  await expect.poll(() => barWidth(page)).toBeGreaterThan(0);
  await page.reload();
  const restored = page.locator("#cards .card").first();
  await expect(restored.locator(".pbar")).toBeVisible();
  await expect.poll(() => barWidth(page)).toBeGreaterThan(0);
  await expect(restored.getByRole("button", { name: "Cancel" })).toBeVisible();
});

test("cancel a running download", async ({ page }) => {
  const card = await fetchUrl(page, "https://slow.example/1");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await card.getByRole("button", { name: "Cancel" }).click();
  await expect(card).toContainText("Download cancelled");
});

test("remove a finished download", async ({ page }) => {
  const card = await fetchUrl(page, "https://ok.example/1");
  await card.getByRole("button", { name: "Download", exact: true }).click();
  await expect(card.getByRole("button", { name: "Save" })).toBeVisible();
  await card.getByRole("button", { name: "Remove" }).click();
  await expect(page.locator("#cards .card")).toHaveCount(0);
  await page.reload();
  await expect(page.locator("#urls")).toBeVisible();
  await expect(page.locator("#cards .card")).toHaveCount(0);
});
