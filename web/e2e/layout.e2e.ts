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

test("phone touch targets are at least 44 px tall", async ({ page }) => {
  test.skip((page.viewportSize()?.width ?? 0) > 560, "phone layout only");
  const tall = async (what: string, locator: ReturnType<typeof page.locator>) => {
    const box = await locator.first().boundingBox();
    expect(box?.height ?? 0, what).toBeGreaterThanOrEqual(44);
  };
  const card = await fetchUrl(page, "https://93.184.216.34/ok");
  await tall("quality chip", card.getByRole("group", { name: "Quality" }).getByRole("button"));
  await tall("download", card.getByRole("button", { name: /^Download/ }));
  await page.getByRole("checkbox", { name: "Clip" }).click();
  await tall("clip from", page.getByLabel("From"));
  await tall("clip to", page.getByLabel("To", { exact: true }));
  await tall("clip label", page.locator("label[for=clip]"));
  await page.getByRole("checkbox", { name: "Clip" }).click();
  await card.getByRole("button", { name: /^Download/ }).click();
  await expect(card.getByRole("link", { name: "Save" })).toBeVisible();
  await tall("save", card.getByRole("link", { name: "Save" }));
  await tall("remove", card.getByRole("button", { name: "Remove" }));
  await tall("clean up", page.getByRole("button", { name: "Clean up" }));
  const row = page.locator("[data-history-row]").first();
  await tall("history save", row.getByRole("link", { name: "Save" }));
  await tall("history remove", row.getByRole("button", { name: "Remove from history" }));

  const slow = await fetchUrl(page, "https://93.184.216.34/slow");
  await slow.getByRole("button", { name: /^Download/ }).click();
  await tall("cancel", slow.getByRole("button", { name: "Cancel" }));
});
