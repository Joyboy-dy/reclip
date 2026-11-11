import { ApiFailure, createJob, fileUrl, getConfig, getJob, probe } from "./api/client";
import { type Card, expandPlaylists, pollJob } from "./state";
import { renderCard, renderDownloadAll } from "./ui/card";
import { renderTelegramLink } from "./ui/footer";
import { setupFormatToggle } from "./ui/format-toggle";
import { setupInput } from "./ui/input";

let cards: Card[] = [];
const getFormat = setupFormatToggle();

const message = (err: unknown): string => (err instanceof Error ? err.message : String(err));
const draw = (idx: number) =>
  renderCard(cards[idx], idx, getFormat(), { download: dlCard, save: saveCard, pick: pickFormat });

function pickFormat(idx: number, formatId: string): void {
  cards[idx].selectedFormat = formatId;
  draw(idx);
}

function saveCard(idx: number): void {
  const c = cards[idx];
  if (!c.jobId) return;
  const a = document.createElement("a");
  a.href = fileUrl(c.jobId);
  a.download = c.filename ?? "";
  a.click();
}

async function dlCard(idx: number): Promise<void> {
  const c = cards[idx];
  c.status = "downloading";
  c.error = undefined;
  draw(idx);
  const format = getFormat();
  try {
    const job = await createJob({
      url: c.url,
      format,
      format_id: format === "mp4" ? (c.selectedFormat ?? null) : null,
      title: c.info?.title ?? null,
    });
    c.jobId = job.id;
    const final = await pollJob(job.id, getJob, () => {});
    if (final.state === "done") {
      c.status = "done";
      c.filename = final.filename ?? undefined;
      draw(idx);
      saveCard(idx);
    } else {
      c.status = "error";
      c.error = final.error?.message ?? "Download failed";
      draw(idx);
    }
  } catch (err) {
    c.status = "error";
    c.error = err instanceof ApiFailure ? err.message : "Lost connection to server";
    draw(idx);
  }
}

async function go(): Promise<void> {
  const urls = input.getUrls();
  if (!urls.length) return;
  input.setBusy(true);
  (document.getElementById("cards") as HTMLElement).innerHTML = "";
  cards = [];

  const all = await expandPlaylists(urls, probe);
  for (const url of all) {
    const idx = cards.length;
    cards.push({ url, status: "loading" });
    draw(idx);
    try {
      const res = await probe(url);
      if (res.kind === "video") {
        cards[idx] = { url, status: "ready", info: res, selectedFormat: res.formats[0]?.id };
      } else {
        cards[idx] = { url, status: "info-error", error: "This URL is not supported" };
      }
    } catch (err) {
      cards[idx] = { url, status: "info-error", error: message(err) };
    }
    draw(idx);
  }

  if (cards.filter((c) => c.status === "ready").length > 1) {
    const btn = renderDownloadAll(async () => {
      btn.disabled = true;
      btn.textContent = "Downloading...";
      for (let i = 0; i < cards.length; i++) {
        if (cards[i].status === "ready") await dlCard(i);
      }
      btn.disabled = false;
      btn.textContent = "Download All";
    });
  }
  input.setBusy(false);
}

const input = setupInput(go);
getConfig()
  .then((cfg) => renderTelegramLink(cfg.telegram_bot_username))
  .catch(() => {});
