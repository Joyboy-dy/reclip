import { ApiFailure, cancelJob, createJob, deleteJob, fileUrl, listJobs, probe, resumeJob } from "../api/client";
import type { Job } from "../api/types.gen";
import { errorText } from "../errors";
import { cappedFollower } from "../live-slots";
import { followJob, pollFollow } from "../progress";
import { rateLimitWait } from "../retry";
import {
  type Card,
  type JobOptions,
  applyJob,
  cardFromJob,
  defaultFormat,
  entryCard,
  followsPageOptions,
  isTerminal,
  jobRequest,
} from "../state";
import { agentJobIds, rememberAgentJob } from "./agentJobs";
import { clipFromInput } from "./format";
import { loadHistory } from "./history";
import { findCard, store, updateCard } from "./store";

/** The API error code of a failure, when the server sent one. */
const errorCodeOf = (err: unknown): string | undefined => (err instanceof ApiFailure ? err.code : undefined);

const stops = new Map<string, () => void>();
let nextKey = 0;
export const newKey = (): string => `c${nextKey++}`;
const sleep = (ms: number) => new Promise<void>((done) => setTimeout(done, ms));

// At most 3 open EventSources per page; other unfinished jobs are polled.
const follow = cappedFollower(
  (id, onJob, onError, onStreamEnd) => followJob(id, onJob, undefined, onError, onStreamEnd),
  (id, onJob, onError) => pollFollow(id, onJob, onError),
);

/**
 * The options a card's download takes, or the reason they cannot be used.
 * Only a card that follows the page options reads (and checks) them: one that
 * keeps its own job's options is never blocked by an invalid page clip.
 */
function optionsFor(c: Card, override?: JobOptions): { opts?: JobOptions; error?: string } {
  if (override) return { opts: override };
  // An agent card retried from the page keeps the agent's own options.
  if (c.autoSave === false && c.format !== undefined) return { opts: { format: c.format, clip: c.clip, strip: c.strip ?? false } };
  const o = store.get().options;
  const format = o.media === "video" ? o.container : o.audio;
  if (!followsPageOptions(c)) return { opts: { format, strip: o.strip } };
  const { clip, error } = clipFromInput({ on: o.clipOn, from: o.clipFrom, to: o.clipTo });
  if (error) return { error };
  return { opts: { format, clip, strip: o.strip } };
}

/** Let the browser save a file through a temporary link. */
export function saveFile(href: string, name = ""): void {
  const a = document.createElement("a");
  a.href = href;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
}

function stopTracking(key: string): void {
  stops.get(key)?.();
  stops.delete(key);
}

/** Follow a job and mirror its live state into its card. */
function track(key: string, jobId: string): void {
  stopTracking(key);
  if (findCard(key)?.autoSave === false) rememberAgentJob(jobId);
  const stop = follow(
    jobId,
    (job: Job) => {
      const wasDone = findCard(key)?.status === "done";
      if (!updateCard(key, (c) => applyJob(c, job))) return;
      if (isTerminal(job.state)) stops.delete(key);
      const c = findCard(key);
      if (c?.status === "done" && !wasDone) {
        if (c.autoSave !== false) saveFile(fileUrl(jobId), c.filename);
        void loadHistory();
      }
    },
    (err) => {
      stops.delete(key);
      updateCard(key, (c) => {
        c.status = "error";
        c.error = errorText(err);
      });
    },
  );
  stops.set(key, stop);
}

/** Start (or restart) the download of a card with the page options, or with `override` (the WebMCP tools). */
export async function download(key: string, override?: JobOptions): Promise<void> {
  const first = findCard(key);
  if (!first) return;
  const { opts, error } = optionsFor(first, override);
  if (!opts) {
    updateCard(key, (c) => {
      c.error = error;
    });
    return;
  }
  const req = jobRequest(first, opts);
  // The previous job (failed or cancelled) stays until its replacement
  // exists: a restored card can only retry through it, so a failed creation
  // must leave it in place (Retry and Remove still work, a refresh keeps it).
  const previous = first.jobId;
  updateCard(key, (c) => {
    Object.assign(c, { status: "downloading", format: req.format, clip: req.clip, strip: req.strip_metadata });
    c.error = c.actionError = c.jobId = c.jobState = c.waitUntil = undefined;
    c.progress = c.speed = c.eta = undefined;
    c.resumable = false;
  });
  if (previous) stopTracking(key);
  const dropPrevious = async () => {
    if (previous) await deleteJob(previous).catch(() => {});
  };
  // A rate-limited creation waits and retries a few times; a card removed
  // meanwhile stops the loop.
  for (let retries = 0; ; retries++) {
    try {
      const job = await createJob(req);
      await dropPrevious();
      if (!updateCard(key, (c) => applyJob(c, job))) {
        await deleteJob(job.id).catch(() => {});
        return;
      }
      if (!isTerminal(job.state)) track(key, job.id);
      return;
    } catch (err) {
      const wait = rateLimitWait(err, retries);
      const alive = updateCard(key, (c) => {
        if (wait !== undefined) c.waitUntil = Date.now() + wait * 1000;
        else Object.assign(c, { status: "error", error: errorText(err), waitUntil: undefined, jobId: previous, errorCode: errorCodeOf(err) });
      });
      if (!alive) await dropPrevious();
      if (!alive || wait === undefined) return;
      await sleep(wait * 1000);
      if (!updateCard(key, (c) => (c.waitUntil = undefined))) {
        await dropPrevious();
        return;
      }
    }
  }
}

export async function downloadAll(): Promise<void> {
  const ready = store.get().cards.filter((c) => c.status === "ready");
  await Promise.all(ready.map((c) => download(c.key)));
}

export async function cancel(key: string): Promise<void> {
  const jobId = findCard(key)?.jobId;
  if (!jobId) return;
  try {
    const job = await cancelJob(jobId);
    updateCard(key, (c) => {
      applyJob(c, job);
      c.actionError = c.actionErrorCode = undefined;
    });
    if (isTerminal(job.state)) stopTracking(key);
  } catch (err) {
    updateCard(key, (c) => Object.assign(c, { actionError: errorText(err), actionErrorCode: errorCodeOf(err) }));
  }
}

/** Continue a stopped job from the part already downloaded. */
export async function resume(key: string): Promise<void> {
  const jobId = findCard(key)?.jobId;
  if (!jobId) return;
  try {
    const job = await resumeJob(jobId);
    updateCard(key, (c) => {
      applyJob(c, job);
      c.actionError = undefined;
    });
    if (!isTerminal(job.state)) track(key, job.id);
  } catch (err) {
    updateCard(key, (c) => (c.actionError = errorText(err)));
  }
}

export async function remove(key: string): Promise<void> {
  const jobId = findCard(key)?.jobId;
  if (jobId) {
    try {
      await deleteJob(jobId);
    } catch (err) {
      updateCard(key, (c) => (c.actionError = errorText(err)));
      return;
    }
  }
  stopTracking(key);
  store.set((s) => ({ cards: s.cards.filter((c) => c.key !== key) }));
  if (jobId) void loadHistory();
}

export const pickFormat = (key: string, formatId: string): void => {
  updateCard(key, (c) => (c.selectedFormat = formatId));
};

/** Probe a card's link: a video becomes ready, a playlist is replaced by one card per entry. */
async function probeCard(key: string, depth = 0): Promise<void> {
  const url = findCard(key)?.url;
  if (!url) return;
  try {
    const res = await probe(url);
    if (res.kind === "video") {
      updateCard(key, (c) => Object.assign(c, { status: "ready", info: res, selectedFormat: defaultFormat(res.formats) }));
      return;
    }
    if (depth > 0 || res.entries.length === 0) {
      const error = depth > 0 ? "This URL is not supported" : "This playlist is empty";
      updateCard(key, (c) => Object.assign(c, { status: "info-error", error }));
      return;
    }
    const entries = res.entries.map((e) => entryCard(e, newKey()));
    store.set((s) => ({ cards: s.cards.flatMap((c) => (c.key === key ? entries : [c])) }));
    for (const e of entries) if (e.status === "loading") await probeCard(e.key, depth + 1);
  } catch (err) {
    updateCard(key, (c) => Object.assign(c, { status: "info-error", error: errorText(err) }));
  }
}

/** Probe pasted links into new cards on top; started cards stay, the others are replaced. Resolves true when a card is ready. */
export async function fetchLinks(urls: string[]): Promise<boolean> {
  if (!urls.length) return false;
  const fresh: Card[] = urls.map((url) => ({ key: newKey(), url, status: "loading" }));
  store.set((s) => ({ busy: true, cards: [...fresh, ...s.cards.filter((c) => c.jobId || c.status === "downloading" || (c.autoSave === false && c.status !== "info-error"))] }));
  for (const c of fresh) await probeCard(c.key);
  store.set({ busy: false });
  return store.get().cards.some((c) => c.status === "ready");
}

/** Add a card that downloads an entry of the history again. */
export function addAndDownload(card: Card): void {
  store.set((s) => ({ cards: [card, ...s.cards] }));
  void download(card.key);
}

/** Bring back the jobs of this visitor after a page load. */
export async function restore(): Promise<void> {
  let jobs: Job[];
  try {
    jobs = await listJobs();
  } catch {
    return;
  }
  const known = new Set(store.get().cards.map((c) => c.key));
  const agent = agentJobIds();
  const restored = jobs
    .filter((j) => j.state !== "expired" && !known.has(j.id))
    .map((j) => (agent.has(j.id) ? { ...cardFromJob(j), autoSave: false } : cardFromJob(j)));
  store.set((s) => ({ cards: [...s.cards, ...restored] }));
  for (const c of restored) if (c.jobId && c.jobState && !isTerminal(c.jobState)) track(c.key, c.jobId);
}
