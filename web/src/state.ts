import { jobError } from "./errors";
import type { Job, JobState, MediaFormat, ProbeResponse, VideoInfo } from "./api/types.gen";

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
  const c: Card = { key: job.id, url: job.url, status: "downloading" };
  applyJob(c, job);
  return c;
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

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Poll a job until it reaches a terminal state; resolves with the final job. */
export async function pollJob(
  id: string,
  getJob: (id: string) => Promise<Job>,
  onUpdate: (job: Job) => void,
  intervalMs = 1500,
  shouldStop: () => boolean = () => false,
): Promise<Job> {
  for (;;) {
    const job = await getJob(id);
    if (!shouldStop()) onUpdate(job);
    if (isTerminal(job.state) || shouldStop()) return job;
    await sleep(intervalMs);
  }
}
