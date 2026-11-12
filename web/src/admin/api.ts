import { ApiFailure } from "@/api/client";
import type {
  AdminLogin,
  AdminMe,
  AdminPasswordChange,
  ApiKeyInfo,
  ApiKeyList,
  CreateKeyRequest,
  CreatedKey,
  ErrorCode,
} from "@/api/types.gen";

const BASE = "/api/v1/admin";

async function send(path: string, method: string, body?: unknown): Promise<Response> {
  const res = await fetch(`${BASE}${path}`, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let code: ErrorCode = "internal";
    let message = `Request failed (${res.status})`;
    try {
      const err = (await res.json()) as { code?: ErrorCode; message?: string };
      if (err.code) code = err.code;
      if (err.message) message = err.message;
    } catch {
      // non-JSON error body: keep defaults
    }
    throw new ApiFailure(code, message, res.status, parseRetryAfter(res.headers.get("Retry-After")));
  }
  return res;
}

/** Integer seconds only; an HTTP-date or garbage means "unknown". */
export function parseRetryAfter(value: string | null): number | undefined {
  const text = value?.trim();
  return text && /^\d+$/.test(text) ? Number(text) : undefined;
}

export const SESSION_EXPIRED = "Your session expired. Sign in again.";

/** True when the session is missing or expired (any 401 except a failed sign-in, which callers handle themselves). */
export const isUnauthorized = (err: unknown): boolean => err instanceof ApiFailure && err.status === 401;

async function json<T>(path: string, method: string, body?: unknown): Promise<T> {
  return (await send(path, method, body)).json() as Promise<T>;
}

export const login = (req: AdminLogin): Promise<AdminMe> => json("/session", "POST", req);
export const logout = async (): Promise<void> => void (await send("/session", "DELETE"));
export const getMe = (): Promise<AdminMe> => json("/me", "GET");
export const changePassword = async (req: AdminPasswordChange): Promise<void> =>
  void (await send("/password", "POST", req));
export const listKeys = async (): Promise<ApiKeyInfo[]> => (await json<ApiKeyList>("/keys", "GET")).keys;
export const createKey = (req: CreateKeyRequest): Promise<CreatedKey> => json("/keys", "POST", req);
export const revokeKey = async (id: string): Promise<void> =>
  void (await send(`/keys/${encodeURIComponent(id)}`, "DELETE"));

/** Message to show for a failed admin call. Wrong credentials and network failures get fixed copy. */
export function adminErrorText(err: unknown, unauthorized = SESSION_EXPIRED): string {
  if (err instanceof ApiFailure) {
    if (err.status === 401 || err.code === "unauthorized") return unauthorized;
    if (err.status === 429 || err.code === "rate_limited") {
      return err.retryAfter === undefined
        ? "Too many attempts. Try again in a moment."
        : `Too many attempts. Try again in ${err.retryAfter} s.`;
    }
    if (err.status === 501) return "This feature is not available on the server yet.";
    return err.message;
  }
  return "Network error. Check your connection.";
}
