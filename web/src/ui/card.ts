import type { MediaFormat } from "../api/types.gen";
import { type Card, canRetry } from "../state";
import { esc, fmtDur } from "../urls";
import { progressBarHtml, updateProgressBar } from "./progress-bar";

const ERROR_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>';
const PLAY_ICON =
  '<div class="no-thumb accent"><svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><polygon points="5 3 19 12 5 21 5 3"/></svg></div>';
const IMAGE_ICON =
  '<div class="no-thumb"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="20" rx="2"/><circle cx="8" cy="8" r="1.5"/><path d="m21 15-5-5L5 21"/></svg></div>';

export type CardActions = {
  download: (key: string) => void;
  save: (key: string) => void;
  pick: (key: string, formatId: string) => void;
  cancel: (key: string) => void;
  remove: (key: string) => void;
};

const cardEl = (key: string): HTMLElement | null =>
  document.querySelector<HTMLElement>(`#cards [data-key="${CSS.escape(key)}"]`);

export function removeCardEl(key: string): void {
  cardEl(key)?.remove();
}

function thumb(c: Card, format: MediaFormat): string {
  if ((c.format ?? format) === "mp3") return PLAY_ICON;
  const src = c.thumbnail ?? c.info?.thumbnail;
  if (src) return `<img src="${esc(src)}" alt="">`;
  return IMAGE_ICON;
}

const removeBtn = (c: Card, key: string): string =>
  c.jobId ? `<button class="card-dl-btn ghost" data-act="remove" data-key="${esc(key)}">Remove</button>` : "";

function actions(c: Card, key: string, format: MediaFormat): string {
  const formats = c.info?.formats ?? [];
  const chips =
    format === "mp4" && formats.length > 1
      ? formats
          .map(
            (f) =>
              `<button class="q-chip${f.id === c.selectedFormat ? " active" : ""}" data-act="pick" data-key="${esc(key)}" data-fid="${esc(f.id)}">${esc(f.label)}</button>`,
          )
          .join("")
      : "";
  switch (c.status) {
    case "ready":
      return `<button class="card-dl-btn" data-act="download" data-key="${esc(key)}">Download</button>${chips}`;
    case "downloading":
      return c.jobId
        ? `<div class="card-progress">${progressBarHtml({ progress: c.progress, speed: c.speed, eta: c.eta, state: c.jobState })}</div>
          <button class="card-dl-btn ghost" data-act="cancel" data-key="${esc(key)}">Cancel</button>${c.error ? `<span class="card-status error">${esc(c.error)}</span>` : ""}`
        : '<span class="card-status downloading"><span class="spin"></span> Downloading...</span>';
    case "done":
      return `<button class="card-dl-btn done" data-act="save" data-key="${esc(key)}">Save</button>
          <span class="card-status done">${esc(c.filename ?? "")}</span>${removeBtn(c, key)}`;
    case "error":
      return `${canRetry(c) ? `<button class="card-dl-btn" data-act="download" data-key="${esc(key)}">Retry</button>` : ""}
          <span class="card-status error">${esc(c.error ?? "Download failed")}</span>${removeBtn(c, key)}`;
    default:
      return "";
  }
}

/**
 * Render (or re-render) a card inside #cards. A new card goes before `before`
 * (default: the end); an existing one is updated in place.
 */
export function renderCard(
  c: Card,
  format: MediaFormat,
  handlers: CardActions,
  before: Element | null = null,
): void {
  const container = document.getElementById("cards") as HTMLElement;
  let el = cardEl(c.key);
  if (!el) {
    el = document.createElement("div");
    el.dataset.key = c.key;
    el.addEventListener("click", (e) => {
      const t = (e.target as HTMLElement).closest<HTMLElement>("[data-act]");
      if (!t || t.dataset.key === undefined) return;
      const k = t.dataset.key;
      if (t.dataset.act === "download") handlers.download(k);
      else if (t.dataset.act === "save") handlers.save(k);
      else if (t.dataset.act === "cancel") handlers.cancel(k);
      else if (t.dataset.act === "remove") handlers.remove(k);
      else if (t.dataset.act === "pick") handlers.pick(k, t.dataset.fid ?? "");
    });
    container.insertBefore(el, before?.parentNode === container ? before : null);
  }

  const hadErr = el.dataset.err ?? "";
  el.dataset.err = c.error ? "1" : "";
  if (c.status === "downloading" && c.jobId && hadErr === el.dataset.err && updateProgressBar(el, { progress: c.progress, speed: c.speed, eta: c.eta, state: c.jobState })) {
    return;
  }

  if (c.status === "loading") {
    el.className = "card";
    el.innerHTML = `
          <div class="card-thumb loading"></div>
          <div class="card-body">
            <div class="skeleton-line medium"></div>
            <div class="skeleton-line short"></div>
          </div>
        `;
    return;
  }

  if (c.status === "info-error") {
    el.className = "card card-error";
    el.innerHTML = `
          <div class="card-thumb">
            <div class="card-error-icon">${ERROR_ICON}</div>
          </div>
          <div class="card-body">
            <div class="card-title error">Could not fetch video</div>
            <div class="card-error-msg">${esc(c.error ?? "")}</div>
            <div class="card-error-url">${esc(c.url)}</div>
          </div>
        `;
    return;
  }

  el.className = "card";
  const info = c.info;
  el.innerHTML = `
        <div class="card-thumb">${thumb(c, format)}</div>
        <div class="card-body">
          <div class="card-title">${esc(c.title || info?.title || "Untitled")}</div>
          <div class="card-meta">${esc(info?.uploader ?? "")}${info?.duration ? " · " + fmtDur(info.duration) : ""}</div>
          <div class="card-actions">${actions(c, c.key, c.format ?? format)}</div>
        </div>
      `;
  if (c.status === "downloading" && c.jobId) {
    updateProgressBar(el, { progress: c.progress, speed: c.speed, eta: c.eta, state: c.jobState });
  }
}

export function renderDownloadAll(onClick: () => void, before: Element | null = null): HTMLButtonElement {
  document.getElementById("dl-all-bar")?.remove();
  const bar = document.createElement("div");
  bar.id = "dl-all-bar";
  bar.className = "dl-all-bar";
  bar.innerHTML = '<button class="dl-all-btn">Download All</button>';
  const btn = bar.firstElementChild as HTMLButtonElement;
  btn.addEventListener("click", onClick);
  const container = document.getElementById("cards") as HTMLElement;
  container.insertBefore(bar, before?.parentNode === container ? before : null);
  return btn;
}
