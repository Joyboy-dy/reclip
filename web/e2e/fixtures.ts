import { type Page, expect, test as base } from "@playwright/test";

type ApiJob = { id: string; state: string; progress: number | null };
const TERMINAL = ["done", "failed", "cancelled", "expired"];
// Mutating API calls need a same-origin Origin header (CSRF guard); page.request sends none.
const ORIGIN = "http://127.0.0.1:18999";

export const apiJobs = async (page: Page): Promise<ApiJob[]> =>
  ((await (await page.request.get("/api/v1/jobs")).json()) as { jobs: ApiJob[] }).jobs;

/**
 * Every spec listens (before navigation) for CSP violations, which browsers
 * report as console errors or page errors, and fails on any of them. After the
 * spec, the only download slot (RECLIP_MAX_CONCURRENT=1) is freed through the
 * API, with the context's cookie, before the next spec starts.
 */
export const test = base.extend<{ cspGuard: void }>({
  cspGuard: [
    async ({ page }, use) => {
      const seen: string[] = [];
      const note = (text: string) => {
        if (text.includes("Content Security Policy")) seen.push(text);
      };
      page.on("console", (m) => note(m.text()));
      page.on("pageerror", (e) => note(e.message));
      await use();
      expect(seen, "CSP violations").toEqual([]);
      for (const job of await apiJobs(page)) {
        if (!TERMINAL.includes(job.state)) {
          await page.request.post(`/api/v1/jobs/${job.id}/cancel`, { data: {}, headers: { Origin: ORIGIN } });
        }
      }
      await expect.poll(async () => (await apiJobs(page)).every((j) => TERMINAL.includes(j.state))).toBe(true);
    },
    { auto: true },
  ],
});

export { expect };

export const cards = (page: Page) => page.locator("[data-card]");

// Public IP literals: the SSRF guard accepts them without DNS, and the fake
// yt-dlp picks its behaviour from the "ok" / "slow" path.
export async function fetchUrl(page: Page, url: string) {
  await page.goto("/");
  await page.locator("#urls").fill(url);
  await page.getByRole("button", { name: "Fetch" }).click();
  const card = cards(page).first();
  await expect(card.getByRole("button", { name: /^Download/ })).toBeVisible();
  return card;
}

/** Nothing sticks out sideways: no horizontal page scroll. */
export async function expectNoOverflow(page: Page) {
  const over = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(over, "horizontal overflow in px").toBeLessThanOrEqual(0);
}
