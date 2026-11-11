import { cancelJob, createJob, deleteJob, fileUrl, getConfig, listJobs, probe } from "./api/client";
import type { Job } from "./api/types.gen";
import { errorText } from "./errors";
import { followJob } from "./progress";
import { type Card, applyJob, cardFromJob, expandPlaylists, isTerminal } from "./state";
import { type CardActions, removeCardEl, renderCard, renderDownloadAll } from "./ui/card";
import { renderTelegramLink } from "./ui/footer";
import { setupFormatToggle } from "./ui/format-toggle";
import { setupInput } from "./ui/input";

let cards: Card[] = [];
const stops = new Map<string, () => void>();
let nextKey = 0;
const getFormat = setupFormatToggle();
const find = (key: string): Card | undefined => cards.find((c) => c.key === key);

const draw = (c: Card, before: Element | null = null) => renderCard(c, getFormat(), actions, before);

function pickFormat(key: string, formatId: string): void {
  const c = find(key);
  if (!c) return;
  c.selectedFormat = formatId;
  draw(c);
}

function saveCard(key: string): void {
  const c = find(key);
  if (!c?.jobId) return;
  const a = document.createElement("a");
  a.href = fileUrl(c.jobId);
  a.download = c.filename ?? "";
  a.click();
}

/** Follow a job and mirror its live state into its card. */
function track(c: Card, jobId: string): void {
  stops.get(c.key)?.();
  const stop = followJob(jobId, (job: Job) => {
    if (!find(c.key)) return;
    const wasDone = c.status === "done";
    applyJob(c, job);
    draw(c);
    if (isTerminal(job.state)) stops.delete(c.key);
    if (c.status === "done" && !wasDone) saveCard(c.key);
  });
  stops.set(c.key, stop);
}

async function dlCard(key: string): Promise<void> {
  const c = find(key);
  if (!c) return;
  const previous = c.jobId;
  c.status = "downloading";
  c.error = undefined;
  c.jobId = undefined;
  c.jobState = undefined;
  c.progress = c.speed = c.eta = undefined;
  draw(c);
  if (previous) {
    stops.get(key)?.();
    stops.delete(key);
    void deleteJob(previous).catch(() => {});
  }
  const format = getFormat();
  try {
    const job = await createJob({
      url: c.url,
      format,
      format_id: format === "mp4" ? c.selectedFormat ?? undefined : undefined,
      title: c.info?.title,
      thumbnail: c.info?.thumbnail || undefined,
    });
    if (!find(key)) return;
    applyJob(c, job);
    draw(c);
    track(c, job.id);
  } catch (err) {
    c.status = "error";
    c.error = errorText(err);
    draw(c);
  }
}

async function cancelCard(key: string): Promise<void> {
  const c = find(key);
  if (!c?.jobId) return;
  try {
    applyJob(c, await cancelJob(c.jobId));
    draw(c);
  } catch (err) {
    c.error = errorText(err);
  }
}

async function removeCard(key: string): Promise<void> {
  const c = find(key);
  if (!c) return;
  if (c.jobId) {
    try {
      await deleteJob(c.jobId);
    } catch (err) {
      c.status = "error";
      c.error = errorText(err);
      draw(c);
      return;
    }
  }
  stops.get(key)?.();
  stops.delete(key);
  cards = cards.filter((x) => x.key !== key);
  removeCardEl(key);
}

const actions: CardActions = {
  download: (k) => void dlCard(k),
  save: saveCard,
  pick: pickFormat,
  cancel: (k) => void cancelCard(k),
  remove: (k) => void removeCard(k),
};

async function go(): Promise<void> {
  const urls = input.getUrls();
  if (!urls.length) return;
  input.setBusy(true);
  // Keep started cards (restored or downloading); drop the un-started ones.
  document.getElementById("dl-all-bar")?.remove();
  for (const c of cards) if (!c.jobId && c.status !== "downloading") removeCardEl(c.key);
  cards = cards.filter((c) => c.jobId || c.status === "downloading");
  const anchor = document.getElementById("cards")?.firstElementChild ?? null;

  const fresh: Card[] = [];
  const all = await expandPlaylists(urls, probe);
  for (const url of all) {
    const c: Card = { key: `c${nextKey++}`, url, status: "loading" };
    cards.push(c);
    fresh.push(c);
    draw(c, anchor);
    try {
      const res = await probe(url);
      if (res.kind === "video") {
        Object.assign(c, { status: "ready", info: res, selectedFormat: res.formats[0]?.id });
      } else {
        Object.assign(c, { status: "info-error", error: "This URL is not supported" });
      }
    } catch (err) {
      Object.assign(c, { status: "info-error", error: errorText(err) });
    }
    draw(c);
  }

  const ready = () => fresh.filter((c) => c.status === "ready");
  if (ready().length > 1) {
    const btn = renderDownloadAll(async () => {
      btn.disabled = true;
      btn.textContent = "Downloading...";
      await Promise.all(ready().map((c) => dlCard(c.key)));
      btn.disabled = false;
      btn.textContent = "Download All";
    }, anchor);
  }
  if (ready().length > 0) input.clearDraft();
  input.setBusy(false);
}

async function restore(): Promise<void> {
  let jobs: Job[];
  try {
    jobs = await listJobs();
  } catch {
    return;
  }
  for (const job of jobs) {
    if (job.state === "expired" || find(job.id)) continue;
    const c = cardFromJob(job);
    cards.push(c);
    draw(c);
    if (!isTerminal(job.state)) track(c, job.id);
  }
}

const input = setupInput(go);
void restore();
getConfig()
  .then((cfg) => renderTelegramLink(cfg.telegram_bot_username))
  .catch(() => {});
