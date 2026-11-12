import type { Job } from "./api/types.gen";
import { isTerminal } from "./state";

/**
 * Live event streams allowed per page (ruling P2-R14). Over HTTP/1.1 a browser
 * opens at most 6 connections per origin and each open EventSource holds one,
 * so one stream per card would stall every other request (Cancel, Remove,
 * save, new jobs) once more than 5 jobs run. Other jobs are polled.
 */
export const MAX_LIVE = 3;

/** Who holds a live slot, and who waits for one (oldest first). */
export class LiveSlots {
  private live = new Set<string>();
  private waiting: string[] = [];

  constructor(readonly cap = MAX_LIVE) {}

  /** Take a free slot (true), or join the line for one (false). */
  acquire(key: string): boolean {
    if (this.live.has(key)) return true;
    if (this.live.size < this.cap) {
      this.live.add(key);
      return true;
    }
    if (!this.waiting.includes(key)) this.waiting.push(key);
    return false;
  }

  /** Leave a slot or the line. Returns the key promoted into the freed slot, if any. */
  release(key: string): string | undefined {
    if (!this.live.delete(key)) {
      this.waiting = this.waiting.filter((k) => k !== key);
      return undefined;
    }
    const next = this.waiting.shift();
    if (next !== undefined) this.live.add(next);
    return next;
  }

  isLive(key: string): boolean {
    return this.live.has(key);
  }

  get waitingKeys(): readonly string[] {
    return this.waiting;
  }
}

export type OnJob = (job: Job) => void;
export type OnError = (err: unknown) => void;
/** Follow over an EventSource; `onStreamEnd` fires once when it closes. Returns a stop function. */
export type LiveStart = (id: string, onJob: OnJob, onError: OnError, onStreamEnd: () => void) => () => void;
/** Follow by polling. Returns a stop function. */
export type PollStart = (id: string, onJob: OnJob, onError: OnError) => () => void;
export type Follow = (id: string, onJob: OnJob, onError: OnError) => () => void;

/**
 * Follow jobs with at most `cap` live streams. A job without a slot is
 * polled; whenever a stream ends (terminal job, stream error, stop) the
 * oldest polled job is moved to a live stream. A follow gives its slot or
 * place back on a terminal job, an error or its stop function.
 */
export function cappedFollower(live: LiveStart, poll: PollStart, cap = MAX_LIVE): Follow {
  const slots = new LiveSlots(cap);
  type Entry = { id: string; onJob: OnJob; onError: OnError; stop: () => void };
  const entries = new Map<string, Entry>();
  let seq = 0;

  const startLive = (key: string, e: Entry) => {
    e.stop = live(e.id, e.onJob, e.onError, () => free(key));
  };
  const free = (key: string) => {
    const next = slots.release(key);
    const e = next === undefined ? undefined : entries.get(next);
    if (next === undefined || !e) return;
    e.stop(); // its polling
    startLive(next, e);
  };

  return (id, onJob, onError) => {
    const key = String(seq++);
    const end = () => {
      if (entries.delete(key)) free(key);
    };
    const e: Entry = {
      id,
      onJob: (job) => {
        if (isTerminal(job.state)) end();
        onJob(job);
      },
      onError: (err) => {
        end();
        onError(err);
      },
      stop: () => {},
    };
    entries.set(key, e);
    if (slots.acquire(key)) startLive(key, e);
    else e.stop = poll(id, e.onJob, e.onError);
    return () => {
      e.stop();
      end();
    };
  };
}
