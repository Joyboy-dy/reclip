// Cards for downloads started by a browser agent (WebMCP). Unlike Fetch they
// never replace existing cards, never touch `busy`, and a multi-video link
// ends as exactly the one card the agent asked for.
import { probe } from "../api/client";
import type { PlaylistEntry } from "../api/types.gen";
import { errorText } from "../errors";
import { defaultFormat } from "../state";
import { newKey } from "./downloads";
import { findCard, store, updateCard } from "./store";

/** An invalid request the agent can fix (surfaced as `invalid_request`). */
export class AgentInputError extends Error {}

const drop = (key: string) => store.set((s) => ({ cards: s.cards.filter((c) => c.key !== key) }));

function fail(key: string, message: string): never {
  drop(key);
  throw new AgentInputError(message);
}

/** The entry asked for: a post item by number, or the n-th entry of a playlist. */
function pick(entries: PlaylistEntry[], item: number | undefined): PlaylistEntry | undefined {
  if (item === undefined) return entries.length === 1 ? entries[0] : undefined;
  const post = entries.find((e) => e.playlist_item === item);
  return post ?? (entries.every((e) => e.playlist_item === null) ? entries[item - 1] : undefined);
}

/**
 * Add one new card on top (all others stay), probe it and leave it ready.
 * Returns its key. API failures are rethrown (the card then shows the error).
 */
export async function prepareAgentCard(url: string, item?: number): Promise<string> {
  const key = newKey();
  store.set((s) => ({ cards: [{ key, url, status: "loading", autoSave: false }, ...s.cards] }));
  const ready = (info: Awaited<ReturnType<typeof probe>>) =>
    info.kind === "video" &&
    updateCard(key, (c) => Object.assign(c, { status: "ready", info, selectedFormat: defaultFormat(info.formats) }));
  try {
    const res = await probe(url);
    if (res.kind === "video") {
      ready(res);
      return key;
    }
    const entry = pick(res.entries, item);
    if (!entry) {
      fail(key, res.entries.length === 0 ? "This playlist is empty" : item === undefined ? "Several videos: pass playlist_item" : `No item ${item} at this link`);
    }
    if (entry.playlist_item !== null) {
      updateCard(key, (c) =>
        Object.assign(c, { status: "ready", playlistItem: entry.playlist_item, title: entry.title ?? `Video ${entry.playlist_item}` }),
      );
      return key;
    }
    updateCard(key, (c) => Object.assign(c, { url: entry.url, title: entry.title ?? undefined }));
    const one = await probe(entry.url);
    if (one.kind !== "video") fail(key, "This URL is not supported");
    ready(one);
    return key;
  } catch (err) {
    if (findCard(key)?.status === "loading") updateCard(key, (c) => Object.assign(c, { status: "info-error", error: errorText(err) }));
    throw err;
  }
}
