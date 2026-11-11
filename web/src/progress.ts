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

/** Follow a job over SSE, falling back to polling on a real error. Returns a stop function. */
export function followJob(
  id: string,
  onJob: (job: Job) => void,
  deps: FollowDeps = { EventSource: globalThis.EventSource as never, getJob: apiGetJob },
  onError: (err: unknown) => void = () => {},
): () => void {
  let stopped = false;
  let finished = false;
  const es = new deps.EventSource(eventsUrl(id));

  const deliver = (job: Job) => {
    if (stopped || finished) return;
    if (isTerminal(job.state)) {
      finished = true;
      es.close();
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
    es.close();
    void pollJob(id, deps.getJob, deliver, deps.intervalMs ?? 1500, () => stopped || finished).catch((err) => {
      if (!stopped && !finished) onError(err);
    });
  });

  return () => {
    stopped = true;
    es.close();
  };
}
