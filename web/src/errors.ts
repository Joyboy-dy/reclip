import { ApiFailure } from "./api/client";
import type { ErrorCode, Job } from "./api/types.gen";

const TEXTS: Partial<Record<ErrorCode, string>> = {
  unsupported_url: "This URL is not supported",
  unavailable: "Video is unavailable or private",
  private: "This video is private",
  geo_blocked: "Video not available in your region",
  timeout: "Request timed out — try again",
  too_large: "File is too large to download",
  too_long: "Video is too long or is a live stream",
  cancelled: "Download cancelled",
  rate_limited: "Too many requests — try again in a moment",
};

/** Map a server error code to a user-facing text. */
export function friendlyError(code: ErrorCode | undefined, message: string): string {
  if (code === "bot_check" || code === "invalid_request") return message;
  const text = code ? TEXTS[code] : undefined;
  if (text) return text;
  if (code === "internal") {
    if (message.includes("HTTP Error 403")) return "Access denied by the platform";
    if (/copyright/i.test(message)) return "Video blocked due to copyright";
  }
  return message.length > 80 ? message.slice(0, 80) + "..." : message;
}

/** Text for a failed API call: server errors by code, anything else is a network failure. */
export function errorText(err: unknown): string {
  if (err instanceof ApiFailure) return friendlyError(err.code, err.message);
  return "Network error — check your connection";
}

/** Text for a job that ended without being done. */
export function jobError(job: Job): string {
  if (job.error) return friendlyError(job.error.code, job.error.message);
  if (job.state === "cancelled") return "Download cancelled";
  if (job.state === "expired") return "File expired — download it again";
  return "Download failed";
}
