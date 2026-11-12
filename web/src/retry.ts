import { ApiFailure } from "./api/client";

/** Retries of a job creation refused by the rate limit, after the first try. */
export const MAX_RATE_RETRIES = 3;
/** Longer waits are not worth holding a card for: show the error instead. */
export const MAX_RATE_WAIT_SECS = 120;

/**
 * Seconds to wait before retrying a failed job creation, or `undefined` to
 * give up and show the error. `retries` is how many retries were already made.
 */
export function rateLimitWait(err: unknown, retries: number): number | undefined {
  if (!(err instanceof ApiFailure) || err.code !== "rate_limited") return undefined;
  const wait = err.retryAfter;
  if (wait === undefined || wait > MAX_RATE_WAIT_SECS || retries >= MAX_RATE_RETRIES) return undefined;
  return wait;
}
