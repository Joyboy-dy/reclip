import type { MediaFormat } from "../api/types.gen";
import type { Card } from "../state";
import { esc, fmtDur } from "../urls";

const ERROR_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>';
const PLAY_ICON =
  '<div class="no-thumb" style="color:var(--accent)"><svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><polygon points="5 3 19 12 5 21 5 3"/></svg></div>';
const IMAGE_ICON =
  '<div class="no-thumb"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="20" rx="2"/><circle cx="8" cy="8" r="1.5"/><path d="m21 15-5-5L5 21"/></svg></div>';

export type CardActions = {
  download: (idx: number) => void;
  save: (idx: number) => void;
  pick: (idx: number, formatId: string) => void;
};

function thumb(c: Card, format: MediaFormat): string {
  if (format === "mp3") return PLAY_ICON;
  if (c.info?.thumbnail) return `<img src="${esc(c.info.thumbnail)}" alt="">`;
  return IMAGE_ICON;
}

function actions(c: Card, idx: number, format: MediaFormat): string {
  const formats = c.info?.formats ?? [];
  const chips =
    format === "mp4" && formats.length > 1
      ? formats
          .map(
            (f) =>
              `<button class="q-chip${f.id === c.selectedFormat ? " active" : ""}" data-act="pick" data-idx="${idx}" data-fid="${esc(f.id)}">${esc(f.label)}</button>`,
          )
          .join("")
      : "";
  switch (c.status) {
    case "ready":
      return `<button class="card-dl-btn" data-act="download" data-idx="${idx}">Download</button>${chips}`;
    case "downloading":
      return '<span class="card-status downloading"><span class="spin"></span> Downloading...</span>';
    case "done":
      return `<button class="card-dl-btn done" data-act="save" data-idx="${idx}">Save</button>
          <span class="card-status done">${esc(c.filename ?? "")}</span>`;
    case "error":
      return `<button class="card-dl-btn" data-act="download" data-idx="${idx}">Retry</button>
          <span class="card-status error">${esc(c.error ?? "Download failed")}</span>`;
    default:
      return "";
  }
}

/** Render (or re-render) the card at `idx` inside #cards. */
export function renderCard(c: Card, idx: number, format: MediaFormat, handlers: CardActions): void {
  const container = document.getElementById("cards") as HTMLElement;
  let el = document.getElementById(`card-${idx}`);
  if (!el) {
    el = document.createElement("div");
    el.id = `card-${idx}`;
    el.addEventListener("click", (e) => {
      const t = (e.target as HTMLElement).closest<HTMLElement>("[data-act]");
      if (!t || t.dataset.idx === undefined) return;
      const i = Number(t.dataset.idx);
      if (t.dataset.act === "download") handlers.download(i);
      else if (t.dataset.act === "save") handlers.save(i);
      else if (t.dataset.act === "pick") handlers.pick(i, t.dataset.fid ?? "");
    });
    container.appendChild(el);
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
            <div class="card-title" style="color:var(--error)">Could not fetch video</div>
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
          <div class="card-title">${esc(info?.title || "Untitled")}</div>
          <div class="card-meta">${esc(info?.uploader ?? "")}${info?.duration ? " · " + fmtDur(info.duration) : ""}</div>
          <div class="card-actions">${actions(c, idx, format)}</div>
        </div>
      `;
}

export function renderDownloadAll(onClick: () => void): HTMLButtonElement {
  document.getElementById("dl-all-bar")?.remove();
  const bar = document.createElement("div");
  bar.id = "dl-all-bar";
  bar.className = "dl-all-bar";
  bar.innerHTML = '<button class="dl-all-btn">Download All</button>';
  const btn = bar.firstElementChild as HTMLButtonElement;
  btn.addEventListener("click", onClick);
  document.getElementById("cards")?.appendChild(bar);
  return btn;
}
