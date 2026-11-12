import { ApiFailure } from "./api/client";
import { jobError } from "./errors";
import type {
  Clip,
  CreateJobRequest,
  FormatOption,
  Job,
  JobState,
  MediaFormat,
  PlaylistEntry,
  VideoInfo,
} from "./api/types.gen";

export const VIDEO_FORMATS = ["mp4", "mkv", "mov"] as const satisfies readonly MediaFormat[];
export const AUDIO_FORMATS = ["mp3", "m4a", "opus", "flac", "wav"] as const satisfies readonly MediaFormat[];
export type VideoFormat = (typeof VIDEO_FORMATS)[number];
export type AudioFormat = (typeof AUDIO_FORMATS)[number];
export const isVideo = (f: MediaFormat): f is VideoFormat => (VIDEO_FORMATS as readonly string[]).includes(f);

export type Card = {
  /** Stable identity (job id for restored cards, generated otherwise); never an array index. */
  key: string;
  url: string;
  status: "loading" | "ready" | "info-error" | "downloading" | "done" | "error";
  info?: VideoInfo;
  /** 1-based item of a multi-video post. */
  playlistItem?: number;
  jobId?: string;
  error?: string;
  selectedFormat?: string;
  filename?: string;
  title?: string;
  thumbnail?: string;
  format?: MediaFormat;
  clip?: Clip;
  strip?: boolean;
  resumable?: boolean;
  size?: number;
  progress?: number;
  speed?: number;
  eta?: number;
  jobState?: JobState;
  /** Epoch ms at which a rate-limited job creation is retried. */
  waitUntil?: number;
  /** Failure of the last Cancel, Resume or Remove request. */
  actionError?: string;
};

const TERMINAL: JobState[] = ["done", "failed", "cancelled", "expired"];
export const isTerminal = (s: JobState): boolean => TERMINAL.includes(s);

/** Copy the live state of a job into its card. A stopped job keeps the last progress seen. */
export function applyJob(c: Card, job: Job): void {
  c.jobId = job.id;
  c.jobState = job.state;
  const stopped = isTerminal(job.state) && job.state !== "done";
  if (!stopped || job.progress != null) c.progress = job.progress ?? undefined;
  c.speed = job.speed ?? undefined;
  c.eta = job.eta ?? undefined;
  if (job.title) c.title = job.title;
  if (job.thumbnail) c.thumbnail = job.thumbnail;
  c.format = job.format;
  c.clip = job.clip ?? undefined;
  c.strip = job.strip_metadata;
  c.resumable = job.resumable;
  if (job.size != null) c.size = job.size;
  if (job.filename) c.filename = job.filename;
  if (job.state === "done") {
    c.status = "done";
    c.error = undefined;
  } else if (isTerminal(job.state)) {
    c.status = "error";
    c.error = jobError(job);
  } else {
    c.status = "downloading";
    c.error = undefined;
  }
}

/** Build a card from a job alone (page refresh). */
export function cardFromJob(job: Job): Card {
  const c: Card = {
    key: job.id,
    url: job.url,
    status: "downloading",
    selectedFormat: job.format_id ?? undefined,
    playlistItem: job.playlist_item ?? undefined,
  };
  applyJob(c, job);
  return c;
}

/** A card can be (re)started from its probe result, its playlist entry or, once restored, from its job. */
export const canRetry = (c: Card): boolean => Boolean(c.info || c.jobId || c.playlistItem);

/** The page-wide choices a new download takes. */
export type JobOptions = { format: MediaFormat; clip?: Clip; strip: boolean };

/**
 * The request that (re)starts a card. A card fetched on this page follows the
 * page options; one that already ran a job (restored, or downloaded again from
 * the history) keeps the format, quality and clip of that job.
 */
export function jobRequest(c: Card, opts: JobOptions): CreateJobRequest {
  const own = !c.info && c.format !== undefined;
  const format = own ? (c.format as MediaFormat) : opts.format;
  return {
    url: c.url,
    format,
    format_id: isVideo(format) ? c.selectedFormat : undefined,
    title: c.info?.title ?? c.title,
    thumbnail: c.info?.thumbnail || c.thumbnail || undefined,
    clip: own ? c.clip : opts.clip,
    strip_metadata: own ? (c.strip ?? opts.strip) : opts.strip,
    playlist_item: c.playlistItem,
  };
}

/** The quality picked by default: the tallest H.264 one, else the tallest. */
export function defaultFormat(formats: FormatOption[]): string | undefined {
  const tallest = (list: FormatOption[]) => list.reduce<FormatOption | undefined>((a, f) => (!a || f.height > a.height ? f : a), undefined);
  return (tallest(formats.filter((f) => f.codec === null)) ?? tallest(formats))?.id;
}

/** Qualities, tallest first. */
export const sortedFormats = (formats: FormatOption[]): FormatOption[] => [...formats].sort((a, b) => b.height - a.height);

/** Qualities for the phone chip row: H.264 ones first (the default leads), then the other codecs; tallest first in each. */
export const h264First = (formats: FormatOption[]): FormatOption[] => {
  const sorted = sortedFormats(formats);
  return [...sorted.filter((f) => f.codec === null), ...sorted.filter((f) => f.codec !== null)];
};

/** A playlist entry as a card: a post item is ready at once, any other entry still has to be probed. */
export function entryCard(entry: PlaylistEntry, key: string): Card {
  if (entry.playlist_item === null) return { key, url: entry.url, status: "loading", title: entry.title ?? undefined };
  return {
    key,
    url: entry.url,
    status: "ready",
    playlistItem: entry.playlist_item,
    title: entry.title ?? `Video ${entry.playlist_item}`,
  };
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** How `pollJob` retries a `getJob` that got no usable answer. */
export type RetryPolicy = {
  /** Waits before the first retries, in order. */
  delaysMs: number[];
  /** Wait before each later retry. */
  everyMs: number;
  /** Give up once requests have kept failing for this long. */
  giveUpMs: number;
};

/** 1.5 s, 3 s, 6 s, then every 10 s, for up to 2 minutes (a server restart). */
export const POLL_RETRY: RetryPolicy = { delaysMs: [1500, 3000, 6000], everyMs: 10_000, giveUpMs: 120_000 };

export type PollOptions = {
  retry?: RetryPolicy;
  sleep?: (ms: number) => Promise<void>;
  now?: () => number;
};

/**
 * Worth retrying: no HTTP answer at all (network down, server restarting) or a
 * 5xx from a proxy in front of a restarting server. A 4xx such as 404 (job
 * removed) will not get better.
 */
const retryable = (err: unknown): boolean => !(err instanceof ApiFailure) || err.status >= 500;

/**
 * Poll a job until it reaches a terminal state; resolves with the final job.
 * Failed requests are retried with backoff (see `POLL_RETRY`); the last error
 * is thrown once retrying gives up, and a 4xx is thrown at once.
 */
export async function pollJob(
  id: string,
  getJob: (id: string) => Promise<Job>,
  onUpdate: (job: Job) => void,
  intervalMs = 1500,
  shouldStop: () => boolean = () => false,
  opts: PollOptions = {},
): Promise<Job> {
  const { retry = POLL_RETRY, sleep: wait = sleep, now = Date.now } = opts;
  let failingSince: number | undefined;
  let failures = 0;
  for (;;) {
    let job: Job;
    try {
      job = await getJob(id);
    } catch (err) {
      if (!retryable(err) || shouldStop()) throw err;
      failingSince ??= now();
      if (now() - failingSince >= retry.giveUpMs) throw err;
      await wait(retry.delaysMs[failures++] ?? retry.everyMs);
      if (shouldStop()) throw err;
      continue;
    }
    failingSince = undefined;
    failures = 0;
    if (!shouldStop()) onUpdate(job);
    if (isTerminal(job.state) || shouldStop()) return job;
    await wait(intervalMs);
  }
}
