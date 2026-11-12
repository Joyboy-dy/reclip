import { cleanHistory, listHistory, removeHistory } from "../api/client";
import type { HistoryClean, HistoryEntry } from "../api/types.gen";
import { errorText } from "../errors";
import { addAndDownload, newKey } from "./downloads";
import { store } from "./store";

export async function loadHistory(): Promise<void> {
  try {
    const entries = await listHistory();
    store.set({ history: { status: "ready", entries } });
  } catch (err) {
    store.set({ history: { status: "error", error: errorText(err) } });
  }
}

/** Keep the list, show why the last action failed. */
function actionFailed(err: unknown): void {
  const h = store.get().history;
  store.set({ history: h.status === "ready" ? { ...h, error: errorText(err) } : { status: "error", error: errorText(err) } });
}

export async function removeEntry(id: string): Promise<void> {
  try {
    await removeHistory(id);
  } catch (err) {
    actionFailed(err);
    return;
  }
  const h = store.get().history;
  if (h.status === "ready") store.set({ history: { status: "ready", entries: h.entries.filter((e) => e.id !== id) } });
}

export async function clean(req: HistoryClean): Promise<void> {
  try {
    await cleanHistory(req);
  } catch (err) {
    actionFailed(err);
    return;
  }
  await loadHistory();
}

/** Start a new download of an entry whose file expired, with its format and quality. */
export function downloadAgain(e: HistoryEntry): void {
  addAndDownload({
    key: newKey(),
    url: e.url,
    status: "ready",
    title: e.title ?? undefined,
    format: e.format,
    selectedFormat: e.format_id ?? undefined,
  });
}
