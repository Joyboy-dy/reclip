import { eventsUrl, getJob as apiGetJob } from "./api/client";
import type { Job } from "./api/types.gen";
import { isTerminal, pollJob } from "./state";

type ES = {
  addEventListener(name: string, fn: (e: { data: string }) => void): void;
  close(): void;
};

export type FollowDeps = {
  EventSource: new (url: string) => ES;
  getJob: (id: string) => Promise<Job>;
  intervalMs?: number;
};

/**
 * Follow a job over SSE, falling back to polling on a real error. Returns a
 * stop function. `onStreamEnd` fires once, when the EventSource is closed
 * (terminal job, stream error or stop), so its connection can be reused.
 */
export function followJob(
  id: string,
  onJob: (job: Job) => void,
  deps: FollowDeps = { EventSource: globalThis.EventSource as never, getJob: apiGetJob },
  onError: (err: unknown) => void = () => {},
  onStreamEnd: () => void = () => {},
): () => void {
  let stopped = false;
  let finished = false;
  let open = true;
  const es = new deps.EventSource(eventsUrl(id));
  const closeStream = () => {
    if (!open) return;
    open = false;
    es.close();
    onStreamEnd();
  };

  const deliver = (job: Job) => {
    if (stopped || finished) return;
    if (isTerminal(job.state)) {
      finished = true;
      closeStream();
    }
    onJob(job);
  };

  es.addEventListener("job", (e) => {
    try {
      deliver(JSON.parse(e.data) as Job);
    } catch {
      // malformed frame: wait for the next one
    }
  });
  es.addEventListener("error", () => {
    // After a terminal job the server closes the stream; that is not a failure.
    if (stopped || finished) return;
    closeStream();
    void pollJob(id, deps.getJob, deliver, deps.intervalMs ?? 1500, () => stopped || finished).catch((err) => {
      if (!stopped && !finished) onError(err);
    });
  });

  return () => {
    stopped = true;
    closeStream();
  };
}

/** Follow a job by polling only (no EventSource). Returns a stop function. */
export function pollFollow(
  id: string,
  onJob: (job: Job) => void,
  onError: (err: unknown) => void = () => {},
  getJob: (id: string) => Promise<Job> = apiGetJob,
  intervalMs = 1500,
): () => void {
  let stopped = false;
  void pollJob(id, getJob, onJob, intervalMs, () => stopped).catch((err) => {
    if (!stopped) onError(err);
  });
  return () => {
    stopped = true;
  };
}
