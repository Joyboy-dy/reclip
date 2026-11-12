import { ApiFailure } from "./api/client";
import { jobError } from "./errors";
import type { CreateJobRequest, Job, JobState, MediaFormat, ProbeResponse, VideoInfo } from "./api/types.gen";

export type Card = {
  /** Stable identity (job id for restored cards, generated otherwise); never an array index. */
  key: string;
  url: string;
  status: "loading" | "ready" | "info-error" | "downloading" | "done" | "error";
  info?: VideoInfo;
  jobId?: string;
  error?: string;
  selectedFormat?: string;
  filename?: string;
  title?: string;
  thumbnail?: string;
  format?: MediaFormat;
  progress?: number;
  speed?: number;
  eta?: number;
  jobState?: JobState;
};

const TERMINAL: JobState[] = ["done", "failed", "cancelled", "expired"];
export const isTerminal = (s: JobState): boolean => TERMINAL.includes(s);

/** Copy the live state of a job into its card. */
export function applyJob(c: Card, job: Job): void {
  c.jobId = job.id;
  c.jobState = job.state;
  c.progress = job.progress ?? undefined;
  c.speed = job.speed ?? undefined;
  c.eta = job.eta ?? undefined;
  if (job.title) c.title = job.title;
  if (job.thumbnail) c.thumbnail = job.thumbnail;
  c.format = job.format;
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
  const c: Card = { key: job.id, url: job.url, status: "downloading", selectedFormat: job.format_id ?? undefined };
  applyJob(c, job);
  return c;
}

/** A card can be (re)started from its probe result or, once restored, from its job. */
export const canRetry = (c: Card): boolean => Boolean(c.info || c.jobId);

/**
 * The request that (re)starts a card. A probed card follows the format toggle;
 * a restored one (no probe result) keeps the format and quality of its job.
 */
export function jobRequest(c: Card, toggle: MediaFormat): CreateJobRequest {
  const format = c.info ? toggle : (c.format ?? toggle);
  return {
    url: c.url,
    format,
    format_id: format === "mp4" ? c.selectedFormat ?? undefined : undefined,
    title: c.info?.title ?? c.title,
    thumbnail: c.info?.thumbnail || c.thumbnail || undefined,
  };
}

/** Replace every playlist URL by the entries the server lists for it. */
export async function expandPlaylists(
  urls: string[],
  probe: (url: string) => Promise<ProbeResponse>,
): Promise<string[]> {
  const out: string[] = [];
  for (const url of urls) {
    if (!url.includes("list=")) {
      out.push(url);
      continue;
    }
    try {
      const res = await probe(url);
      if (res.kind === "playlist") out.push(...res.entries);
      else out.push(url);
    } catch {
      out.push(url);
    }
  }
  return out;
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
