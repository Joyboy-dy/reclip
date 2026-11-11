import type { Job, JobState, ProbeResponse, VideoInfo } from "./api/types.gen";

export type Card = {
  url: string;
  status: "loading" | "ready" | "info-error" | "downloading" | "done" | "error";
  info?: VideoInfo;
  jobId?: string;
  error?: string;
  selectedFormat?: string;
  filename?: string;
};

const TERMINAL: JobState[] = ["done", "failed", "cancelled", "expired"];
export const isTerminal = (s: JobState): boolean => TERMINAL.includes(s);

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
): Promise<Job> {
  for (;;) {
    const job = await getJob(id);
    onUpdate(job);
    if (isTerminal(job.state)) return job;
    await sleep(intervalMs);
  }
}
