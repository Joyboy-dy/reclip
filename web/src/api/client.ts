import type { CreateJobRequest, ErrorCode, Job, ProbeResponse, PublicConfig } from "./types.gen";

export class ApiFailure extends Error {
  constructor(
    public code: ErrorCode,
    message: string,
    public status: number,
  ) {
    super(message);
    this.name = "ApiFailure";
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
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
    throw new ApiFailure(code, message, res.status);
  }
  return (await res.json()) as T;
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
