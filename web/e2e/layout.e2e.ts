import { cards, expect, expectNoOverflow, fetchUrl, test } from "./fixtures";

// Run by every project (desktop, tablet 768×1024, mobile 390×844).

test("no horizontal overflow, empty and with cards", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("ReClip");
  await expectNoOverflow(page);

  const card = await fetchUrl(page, "https://93.184.216.34/ok");
  await page.getByRole("checkbox", { name: "Clip" }).click();
  await expectNoOverflow(page);
  await page.getByRole("radio", { name: "Audio" }).click();
  await expect(page.getByRole("radio", { name: "FLAC" })).toBeVisible();
  await expectNoOverflow(page);
  await card.getByRole("button", { name: /^Download/ }).click();
  await expect(card.getByRole("link", { name: "Save" })).toBeVisible();
  await expectNoOverflow(page);

  await page.getByRole("button", { name: "Clean up" }).click();
  await expect(page.getByRole("menuitem", { name: "Clear all history" })).toBeVisible();
  await expectNoOverflow(page);
});

test("phone layout below 560 px, desktop layout above", async ({ page }) => {
  await page.goto("/");
  const fetch = page.getByRole("button", { name: "Fetch" });
  const width = (await fetch.boundingBox())?.width ?? 0;
  const viewport = page.viewportSize()?.width ?? 0;
  if (viewport <= 560) {
    // Full-width 48 px call to action under the options (.m-cta), 44 px segment cells.
    expect(width).toBeGreaterThan(viewport - 40);
    expect((await fetch.boundingBox())?.height).toBe(48);
    expect((await page.getByRole("radio", { name: "Video" }).boundingBox())?.height).toBe(44);
  } else {
    // Fetch shares the controls row with the segments.
    expect(width).toBeLessThan(560);
  }
  await expect(cards(page)).toHaveCount(0);
});
