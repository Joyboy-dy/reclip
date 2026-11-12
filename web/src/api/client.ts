import type { CreateJobRequest, ErrorCode, Job, ProbeResponse, PublicConfig } from "./types.gen";

export class ApiFailure extends Error {
  constructor(
    public code: ErrorCode,
    message: string,
    public status: number,
    public retryAfter?: number,
  ) {
    super(message);
    this.name = "ApiFailure";
  }
}

/** Integer seconds only; an HTTP-date or garbage means "unknown". */
function parseRetryAfter(value: string | null): number | undefined {
  if (value === null || !/^\d+$/.test(value.trim())) return undefined;
  return Number(value.trim());
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await send(path, init);
  return (await res.json()) as T;
}

async function send(path: string, init?: RequestInit): Promise<Response> {
  const res = await fetch(path, { credentials: "same-origin", ...init });
  if (!res.ok) {
    let code: ErrorCode = "internal";
    let message = `Request failed (${res.status})`;
    try {
      const body = (await res.json()) as { code?: ErrorCode; message?: string };
      if (body.code) code = body.code;
      if (body.message) message = body.message;
    } catch {
      // non-JSON error body: keep defaults
    }
    throw new ApiFailure(code, message, res.status, parseRetryAfter(res.headers.get("Retry-After")));
  }
  return res;
}

function postJson<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

export const getConfig = (): Promise<PublicConfig> => request("/api/v1/config");
export const probe = (url: string): Promise<ProbeResponse> => postJson("/api/v1/probe", { url });
export const createJob = (req: CreateJobRequest): Promise<Job> => postJson("/api/v1/jobs", req);
export const getJob = (id: string): Promise<Job> => request(`/api/v1/jobs/${encodeURIComponent(id)}`);
export const fileUrl = (id: string): string => `/api/v1/jobs/${encodeURIComponent(id)}/file`;
export const listJobs = async (): Promise<Job[]> =>
  (await request<{ jobs: Job[] }>("/api/v1/jobs")).jobs;
export const cancelJob = (id: string): Promise<Job> =>
  postJson(`/api/v1/jobs/${encodeURIComponent(id)}/cancel`, {});
export const deleteJob = async (id: string): Promise<void> => {
  await send(`/api/v1/jobs/${encodeURIComponent(id)}`, { method: "DELETE" });
};
export const eventsUrl = (id: string): string => `/api/v1/jobs/${encodeURIComponent(id)}/events`;
