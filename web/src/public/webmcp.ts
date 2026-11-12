// WebMCP: registers the shared agent tools (src/api/tools.gen.json) with the
// browser's model context, so an in-browser agent drives the page like the
// visitor: same cookie, same limits, same cards. API shape (webmachinelearning/webmcp):
// `document.modelContext.registerTool({ name, description, inputSchema, execute })`
// where `execute(input, options)` returns `{ content: [{ type: "text", text }] }`.
// `navigator.modelContext` (earlier Chrome builds) is accepted as a fallback.
import { ApiFailure, cancelJob, fileUrl, getJob, getTranscript, listJobs, probe } from "../api/client";
import tools from "../api/tools.gen.json";
import type { Job, MediaFormat } from "../api/types.gen";
import { errorText } from "../errors";
import { type Card, type JobOptions, isVideo } from "../state";
import { AgentInputError, prepareAgentCard } from "./agentCards";
import { cancel, download, pickFormat, remove } from "./downloads";
import { findCard, store } from "./store";
import { type Schema, validate } from "./webmcp-schema";

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
const invalid = (message: string) => new ToolError("invalid_request", message);

const ok = (value: unknown): ToolResult => ({ content: [{ type: "text", text: JSON.stringify(value) }] });
const fail = (code: string, message: string): ToolResult => ({
  isError: true,
  content: [{ type: "text", text: JSON.stringify({ code, message }) }],
});

/** A download as the agent sees it; `download_url` (relative) only once the file is ready. */
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
    ...(j.state === "done" ? { download_url: fileUrl(j.id) } : {}),
  };
}

/** A card with no job yet (probing, ready, or waiting for a rate limit): its key is its id. */
const pending = (c: Card) => ({
  id: c.key,
  url: c.url,
  state: "pending",
  status: c.status,
  title: c.title ?? c.info?.title,
  error: c.error,
});

const text = (input: Input, key: string): string => (input[key] as string).trim();
const optional = (input: Input, key: string): string | undefined => (input[key] === undefined ? undefined : text(input, key));

/** Options of start_download, already schema-checked. */
function jobOptions(input: Input): JobOptions {
  const format = (optional(input, "format") ?? "mp4") as MediaFormat;
  const clip = input.clip as { start: number; end?: number } | undefined;
  if (clip && clip.end !== undefined && clip.end <= clip.start) throw invalid('"clip.end" must be greater than "clip.start"');
  if (input.quality !== undefined && !isVideo(format)) throw invalid('"quality" only applies to video formats');
  return { format, strip: input.strip_metadata === true, clip: clip && { start: clip.start, end: clip.end } };
}

/** Resolves once the download of a card has a job, failed, or waits for a rate limit. */
function settled(key: string): Promise<void> {
  const done = () => {
    const c = findCard(key);
    return !c || c.jobId !== undefined || c.status === "error" || c.waitUntil !== undefined;
  };
  return new Promise((resolve) => {
    const check = () => {
      if (!done()) return;
      unsubscribe();
      resolve();
    };
    const unsubscribe = store.subscribe(check);
    check();
  });
}

async function startDownload(input: Input): Promise<ToolResult> {
  const opts = jobOptions(input);
  const quality = optional(input, "quality");
  const url = text(input, "url");
  const key = await prepareAgentCard(url, input.playlist_item as number | undefined);
  const formats = findCard(key)?.info?.formats;
  if (quality && formats && !formats.some((f) => f.id === quality)) {
    await remove(key);
    throw invalid(`Unknown quality "${quality}"; use a format id from probe_media`);
  }
  if (quality) pickFormat(key, quality);
  void download(key, opts);
  await settled(key);
  const c = findCard(key);
  if (!c) throw new ToolError("internal", "The download was removed");
  if (c.status === "error") throw new ToolError(c.errorCode ?? "internal", c.error ?? "The download could not be started");
  if (c.jobId) return ok({ id: c.jobId, state: c.jobState ?? "queued", url, title: c.title ?? c.info?.title });
  return ok({ id: key, state: "waiting", url, message: "Rate limited: the download starts by itself when the limit lifts", retry_at: c.waitUntil });
}

async function cancelDownload(input: Input): Promise<ToolResult> {
  const id = text(input, "id");
  const card = store.get().cards.find((c) => c.jobId === id || c.key === id);
  if (!card) return ok(summary(await cancelJob(id)));
  if (!card.jobId) {
    if (card.autoSave !== false) throw invalid("Not a download");
    await remove(card.key);
    return ok({ id, state: "cancelled" });
  }
  await cancel(card.key);
  const after = findCard(card.key);
  if (after?.actionError) throw new ToolError(after.actionErrorCode ?? "internal", after.actionError);
  return ok({ id, state: after?.jobState ?? "cancelled" });
}

async function listDownloads(): Promise<ToolResult> {
  const jobs = (await listJobs()).map(summary);
  const waiting = store
    .get()
    .cards.filter((c) => !c.jobId && ["loading", "ready", "downloading"].includes(c.status))
    .map(pending);
  return ok({ jobs: [...waiting, ...jobs] });
}

async function getDownload(input: Input): Promise<ToolResult> {
  const id = text(input, "id");
  const card = findCard(id);
  if (card && !card.jobId) return ok(pending(card));
  return ok(summary(await getJob(card?.jobId ?? id)));
}

const handlers: Record<string, (input: Input) => Promise<ToolResult>> = {
  probe_media: async (i) => ok(await probe(text(i, "url"))),
  start_download: startDownload,
  list_downloads: listDownloads,
  get_download: getDownload,
  cancel_download: cancelDownload,
  get_transcript: async (i) => ok(await getTranscript(text(i, "url"), optional(i, "language"))),
};

async function run(name: string, schema: Schema, input: unknown): Promise<ToolResult> {
  try {
    const problem = validate(input ?? {}, schema);
    if (problem) throw invalid(problem);
    return await handlers[name]((input ?? {}) as Input);
  } catch (err) {
    if (err instanceof AgentInputError) return fail("invalid_request", err.message);
    if (err instanceof ToolError || err instanceof ApiFailure) return fail(err.code, err.message);
    return fail("internal", errorText(err));
  }
}

export const toolDescriptors = (): ToolDescriptor[] =>
  tools.map((t) => ({
    name: t.name,
    description: t.description,
    inputSchema: t.inputSchema,
    execute: (input) => run(t.name, t.inputSchema as unknown as Schema, input),
  }));

const swallow = (r: unknown) => {
  if (r && typeof (r as Promise<unknown>).catch === "function") (r as Promise<unknown>).catch(() => {});
};

/** Register after the config bootstrap, and only when the public page is on (else every call would be a 401). */
export function registerWhenPublic(config: { public_web: boolean }): boolean {
  return config.public_web ? registerWebMcp() : false;
}

/** Register the tools when the browser offers WebMCP; never throws. True when at least one tool was registered. */
export function registerWebMcp(ctx?: ModelContextLike): boolean {
  try {
    const g = globalThis as { navigator?: { modelContext?: ModelContextLike }; document?: { modelContext?: ModelContextLike } };
    const mc = ctx ?? g.document?.modelContext ?? g.navigator?.modelContext;
    if (!mc) return false;
    const list = toolDescriptors();
    if (typeof mc.registerTool === "function") {
      let registered = 0;
      for (const t of list) {
        try {
          swallow(mc.registerTool(t));
          registered++;
        } catch {
          // one tool failing must not hide the others
        }
      }
      return registered > 0;
    }
    if (typeof mc.provideContext === "function") {
      swallow(mc.provideContext({ tools: list }));
      return true;
    }
  } catch {
    // WebMCP is optional: never break the page
  }
  return false;
}
