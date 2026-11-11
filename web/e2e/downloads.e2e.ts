import { type Page, expect, test } from "@playwright/test";

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
  await expect(card.locator(".pbar-text")).toHaveText(/\d+ %/);
  await page.reload();
  const restored = page.locator("#cards .card").first();
  await expect(restored.locator(".pbar")).toBeVisible();
  await expect(restored.locator(".pbar-text")).toHaveText(/\d+ %/);
  // Free the single download slot (RECLIP_MAX_CONCURRENT=1) for the next specs.
  await restored.getByRole("button", { name: "Cancel" }).click();
  await expect(restored).toContainText("Download cancelled");
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
