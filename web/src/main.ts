import { createJob, fileUrl, getConfig, getJob, probe } from "./api/client";
import { errorText, jobError } from "./errors";
import { type Card, expandPlaylists, pollJob } from "./state";
import { renderCard, renderDownloadAll } from "./ui/card";
import { renderTelegramLink } from "./ui/footer";
import { setupFormatToggle } from "./ui/format-toggle";
import { setupInput } from "./ui/input";

let cards: Card[] = [];
const getFormat = setupFormatToggle();

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
  let jobId: string;
  try {
    const job = await createJob({
      url: c.url,
      format,
      format_id: format === "mp4" ? c.selectedFormat ?? undefined : undefined,
      title: c.info?.title,
    });
    jobId = job.id;
    c.jobId = jobId;
  } catch (err) {
    c.status = "error";
    c.error = errorText(err);
    draw(idx);
    return;
  }
  // Poll in the background so "Download all" can run every card concurrently.
  void pollJob(jobId, getJob, () => {})
    .then((final) => {
      if (final.state === "done") {
        c.status = "done";
        c.filename = final.filename ?? undefined;
        draw(idx);
        saveCard(idx);
      } else {
        c.status = "error";
        c.error = jobError(final);
        draw(idx);
      }
    })
    .catch((err) => {
      c.status = "error";
      c.error = errorText(err);
      draw(idx);
    });
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
      cards[idx] = { url, status: "info-error", error: errorText(err) };
    }
    draw(idx);
  }

  if (cards.filter((c) => c.status === "ready").length > 1) {
    const btn = renderDownloadAll(async () => {
      btn.disabled = true;
      btn.textContent = "Downloading...";
      const starts: Promise<void>[] = [];
      for (let i = 0; i < cards.length; i++) {
        if (cards[i].status === "ready") starts.push(dlCard(i));
      }
      await Promise.all(starts);
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
