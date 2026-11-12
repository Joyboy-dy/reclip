import { useSyncExternalStore } from "react";
import type { HistoryEntry, PublicConfig } from "../api/types.gen";
import type { AudioFormat, Card, VideoFormat } from "../state";

export type Media = "video" | "audio";

/** Page-wide choices applied to the next downloads. */
export type Options = {
  media: Media;
  container: VideoFormat;
  audio: AudioFormat;
  clipOn: boolean;
  clipFrom: string;
  clipTo: string;
  strip: boolean;
};

export type HistoryState =
  | { status: "loading" }
  | { status: "ready"; entries: HistoryEntry[]; error?: string }
  | { status: "error"; error: string };

export type PageState = {
  cards: Card[];
  options: Options;
  /** A Fetch is in progress. */
  busy: boolean;
  history: HistoryState;
  config?: PublicConfig;
};

export const initialState = (): PageState => ({
  cards: [],
  options: { media: "video", container: "mp4", audio: "mp3", clipOn: false, clipFrom: "", clipTo: "", strip: false },
  busy: false,
  history: { status: "loading" },
});

/** A minimal external store: immutable snapshots, shallow patches. */
export class Store<T> {
  private listeners = new Set<() => void>();
  constructor(private state: T) {}

  get = (): T => this.state;

  set(patch: Partial<T> | ((s: T) => Partial<T>)): void {
    const next = typeof patch === "function" ? patch(this.state) : patch;
    this.state = { ...this.state, ...next };
    for (const l of this.listeners) l();
  }

  subscribe = (l: () => void): (() => void) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };
}

export const store = new Store<PageState>(initialState());

/** Subscribe a component to one slice of the page state (compare by reference). */
export function usePage<S>(select: (s: PageState) => S): S {
  return useSyncExternalStore(store.subscribe, () => select(store.get()));
}

export function setOptions(patch: Partial<Options>): void {
  store.set((s) => ({ options: { ...s.options, ...patch } }));
}

export const findCard = (key: string): Card | undefined => store.get().cards.find((c) => c.key === key);

/** Replace a card by an edited copy; false when the card is gone. */
export function updateCard(key: string, edit: (c: Card) => void): boolean {
  const cards = store.get().cards;
  const i = cards.findIndex((c) => c.key === key);
  if (i < 0) return false;
  const copy = { ...cards[i] } as Card;
  edit(copy);
  store.set({ cards: cards.map((c, j) => (j === i ? copy : c)) });
  return true;
}
