import { cards, expect, test } from "./fixtures";

test("no console errors without WebMCP, and no origin-trial tag without a token", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("ReClip");
  expect(await page.evaluate(() => "modelContext" in navigator || "modelContext" in document)).toBe(false);
  await expect(page.locator('meta[http-equiv="origin-trial"]')).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("a browser agent can start a download through the registered tools", async ({ page }) => {
  await page.addInitScript(() => {
    const tools: Record<string, (i: unknown) => Promise<{ content: { text: string }[] }>> = {};
    (window as unknown as { __tools: typeof tools }).__tools = tools;
    Object.defineProperty(document, "modelContext", {
      value: { registerTool: (t: { name: string; execute: (i: unknown) => Promise<{ content: { text: string }[] }> }) => void (tools[t.name] = t.execute) },
    });
  });
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => Object.keys((window as never as { __tools: object }).__tools).length)).toBe(6);
  const text = await page.evaluate(async () => {
    const t = (window as never as { __tools: Record<string, (i: unknown) => Promise<{ content: { text: string }[] }>> }).__tools;
    return (await t.start_download({ url: "https://93.184.216.34/ok" })).content[0].text;
  });
  expect(JSON.parse(text).id).toBeTruthy();
  await expect(cards(page).first()).toBeVisible();
});
