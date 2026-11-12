// WebMCP: registers the shared agent tools (src/api/tools.gen.json) with the
// browser's model context, so an in-browser agent drives the page like the
// visitor: same cookie, same limits, same cards. API shape (webmachinelearning/webmcp):
// `document.modelContext.registerTool({ name, description, inputSchema, execute })`
// where `execute(input, options)` returns `{ content: [{ type: "text", text }] }`.
// `navigator.modelContext` (earlier Chrome builds) is accepted too.
import { ApiFailure, cancelJob, fileUrl, getJob, getTranscript, listJobs, probe } from "../api/client";
import tools from "../api/tools.gen.json";
import type { Job, MediaFormat } from "../api/types.gen";
import { errorText } from "../errors";
import { type JobOptions, isVideo } from "../state";
import { cancel, download, fetchLinks, pickFormat } from "./downloads";
import { findCard, store } from "./store";

type Input = Record<string, unknown>;
type ToolResult = { content: { type: "text"; text: string }[]; isError?: boolean };
type ToolDescriptor = {
  name: string;
  description: string;
  inputSchema: unknown;
  execute: (input: Input) => Promise<ToolResult>;
};
export type ModelContextLike = {
  registerTool?: (tool: ToolDescriptor, options?: unknown) => unknown;
  provideContext?: (ctx: { tools: ToolDescriptor[] }) => unknown;
};

class ToolError extends Error {
  constructor(
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

const ok = (value: unknown): ToolResult => ({ content: [{ type: "text", text: JSON.stringify(value) }] });
const fail = (code: string, message: string): ToolResult => ({
  isError: true,
  content: [{ type: "text", text: JSON.stringify({ code, message }) }],
});

/** A download as the agent sees it; `file_url` only once the file is ready. */
function summary(j: Job) {
  return {
    id: j.id,
    url: j.url,
    state: j.state,
    title: j.title,
    format: j.format,
    progress: j.progress,
    speed: j.speed,
    eta: j.eta,
    error: j.error,
    ...(j.state === "done" ? { file_url: fileUrl(j.id) } : {}),
  };
}

function str(input: Input, key: string): string;
function str(input: Input, key: string, required: false): string | undefined;
function str(input: Input, key: string, required = true): string | undefined {
  const v = input[key];
  if (typeof v === "string" && v.trim()) return v.trim();
  if (v === undefined && !required) return undefined;
  throw new ToolError("invalid_request", `"${key}" must be a non-empty string`);
}

function jobOptions(input: Input): JobOptions {
  const format = (str(input, "format", false) ?? "mp4") as MediaFormat;
  const clip = input.clip as { start?: unknown; end?: unknown } | undefined;
  if (clip !== undefined && (typeof clip?.start !== "number" || clip.start < 0)) {
    throw new ToolError("invalid_request", '"clip.start" must be a number of seconds');
  }
  return {
    format,
    strip: input.strip_metadata === true,
    clip: clip && { start: clip.start as number, end: typeof clip.end === "number" ? clip.end : undefined },
  };
}

async function startDownload(input: Input): Promise<ToolResult> {
  const url = str(input, "url");
  const opts = jobOptions(input);
  const item = input.playlist_item;
  const known = new Set(store.get().cards.map((c) => c.key));
  // Same path as the Fetch button: a visible card is probed, then downloaded.
  await fetchLinks([url]);
  const fresh = store.get().cards.filter((c) => !known.has(c.key));
  const failed = fresh.find((c) => c.status === "info-error");
  const ready = fresh.filter((c) => c.status === "ready");
  const card = typeof item === "number" ? ready.find((c) => c.playlistItem === item) : ready[0];
  if (!card) {
    throw new ToolError("invalid_request", failed?.error ?? "No downloadable video found at this link");
  }
  if (typeof item !== "number" && ready.length > 1) {
    const items = ready.map((c) => c.playlistItem ?? c.url).join(", ");
    throw new ToolError("invalid_request", `Several videos at this link; pass playlist_item (${items})`);
  }
  const quality = str(input, "quality", false);
  if (quality && isVideo(opts.format)) {
    if (!card.info?.formats.some((f) => f.id === quality)) {
      throw new ToolError("invalid_request", `Unknown quality "${quality}"; use a format id from probe_media`);
    }
    pickFormat(card.key, quality);
  }
  await download(card.key, opts);
  const done = findCard(card.key);
  if (!done?.jobId) throw new ToolError("internal", done?.error ?? "The download could not be started");
  return ok({ id: done.jobId, state: done.jobState ?? "queued", url, title: done.title ?? done.info?.title });
}

async function cancelDownload(input: Input): Promise<ToolResult> {
  const id = str(input, "id");
  const card = store.get().cards.find((c) => c.jobId === id);
  if (!card) return ok(summary(await cancelJob(id)));
  await cancel(card.key);
  const after = findCard(card.key);
  if (after?.actionError) throw new ToolError("internal", after.actionError);
  return ok({ id, state: after?.jobState ?? "cancelled" });
}

const handlers: Record<string, (input: Input) => Promise<ToolResult>> = {
  probe_media: async (i) => ok(await probe(str(i, "url"))),
  start_download: startDownload,
  list_downloads: async () => ok({ downloads: (await listJobs()).map(summary) }),
  get_download: async (i) => ok(summary(await getJob(str(i, "id")))),
  cancel_download: cancelDownload,
  get_transcript: async (i) => ok(await getTranscript(str(i, "url"), str(i, "language", false))),
};

async function run(name: string, input: Input): Promise<ToolResult> {
  try {
    return await handlers[name](input ?? {});
  } catch (err) {
    if (err instanceof ToolError || err instanceof ApiFailure) return fail(err.code, err.message);
    return fail("internal", errorText(err));
  }
}

export const toolDescriptors = (): ToolDescriptor[] =>
  tools.map((t) => ({
    name: t.name,
    description: t.description,
    inputSchema: t.inputSchema,
    execute: (input) => run(t.name, input),
  }));

/** Register the tools when the browser offers WebMCP; never throws. */
export function registerWebMcp(ctx?: ModelContextLike): boolean {
  try {
    const g = globalThis as { navigator?: { modelContext?: ModelContextLike }; document?: { modelContext?: ModelContextLike } };
    const mc = ctx ?? g.navigator?.modelContext ?? g.document?.modelContext;
    if (!mc) return false;
    const list = toolDescriptors();
    const swallow = (r: unknown) => {
      if (r && typeof (r as Promise<unknown>).catch === "function") (r as Promise<unknown>).catch(() => {});
    };
    if (typeof mc.registerTool === "function") {
      for (const t of list) swallow(mc.registerTool(t));
    } else if (typeof mc.provideContext === "function") {
      swallow(mc.provideContext({ tools: list }));
    } else return false;
    return true;
  } catch {
    return false;
  }
}
