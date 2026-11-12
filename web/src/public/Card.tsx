import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { fileUrl } from "../api/client";
import { type Card, canRetry, h264First, isVideo, sortedFormats } from "../state";
import { cardMeta, cardTitle, doneNote, phaseOf, stoppedNote, waitLeft } from "./describe";
import { cancel, download, pickFormat, remove, resume } from "./downloads";
import { formatProgress, progressWidth } from "./format";
import { ClockIcon } from "./icons";
import { Chips, ProgressBar, Thumb, useTick } from "./parts";
import { usePage } from "./store";

export const note = "text-[11px] text-muted";
export const noteError = "text-[11px] text-error";

/** Quality chips are offered for video downloads with more than one quality. */
export function useQualities(c: Card, phone = false) {
  const media = usePage((s) => s.options.media);
  const formats = c.info?.formats ?? [];
  if (media !== "video" || formats.length < 2) return [];
  return phone ? h264First(formats) : sortedFormats(formats);
}

export const isAudioCard = (c: Card, media: string): boolean => (c.format ? !isVideo(c.format) : media === "audio");

/** Cancel / Resume / Remove failures, under the actions. */
export const ActionError = ({ c }: { c: Card }) => (c.actionError ? <span className={noteError}>{c.actionError}</span> : null);

/** One link on desktop and tablet (.rc-card). */
export function CardView({ c }: { c: Card }) {
  const phase = phaseOf(c);
  const media = usePage((s) => s.options.media);
  useTick(phase === "waiting");

  if (phase === "loading") {
    return (
      <article data-card className="flex gap-3.5 rounded-card border-[1.5px] border-line bg-card p-3.5" aria-busy="true">
        <Thumb className="h-20 w-[120px] animate-pulse rounded-lg" />
        <div className="flex min-w-0 flex-1 flex-col justify-center gap-2">
          <div className="h-3.5 w-3/5 animate-pulse rounded bg-line-soft" />
          <div className="h-2.5 w-2/5 animate-pulse rounded bg-line-soft" />
          <span className="sr-only">Fetching {c.url}</span>
        </div>
      </article>
    );
  }

  return (
    <article data-card className="flex gap-3.5 rounded-card border-[1.5px] border-line bg-card p-3.5">
      <Thumb src={c.thumbnail ?? c.info?.thumbnail} audio={isAudioCard(c, media)} className="h-20 w-[120px] rounded-lg" />
      <div className="flex min-w-0 flex-1 flex-col justify-center">
        {phase === "info-error" ? (
          <>
            <div className="mb-0.5 truncate font-serif text-[16px] leading-[1.3] text-error">Could not fetch video</div>
            <div className="mb-2 text-[11px] tracking-[0.03em] text-muted uppercase">{c.error}</div>
            <div className={cn(note, "truncate")}>{c.url}</div>
          </>
        ) : (
          <>
            <div className="mb-0.5 truncate font-serif text-[16px] leading-[1.3]">{cardTitle(c)}</div>
            <div className="mb-2 text-[11px] tracking-[0.03em] text-muted uppercase">{cardMeta(c)}</div>
            <Body c={c} />
          </>
        )}
      </div>
    </article>
  );
}

const Actions = ({ children }: { children: ReactNode }) => <div className="flex flex-wrap items-center gap-1.5">{children}</div>;

function Body({ c }: { c: Card }) {
  const qualities = useQualities(c);
  const phase = phaseOf(c);
  switch (phase) {
    case "ready":
      return (
        <Actions>
          <Button type="button" onClick={() => void download(c.key)}>
            Download
          </Button>
          {qualities.length > 0 && (
            <Chips options={qualities} value={c.selectedFormat} onPick={(id) => pickFormat(c.key, id)} className="contents" />
          )}
          {c.error && <span className={noteError}>{c.error}</span>}
        </Actions>
      );
    case "waiting":
      return (
        <span className="inline-flex items-center gap-1.5 text-[11px] text-accent-ink">
          <ClockIcon />
          Waiting {waitLeft(c)} s (rate limit), then it starts by itself
        </span>
      );
    case "starting":
    case "running":
      return (
        <div className="flex items-center gap-2.5">
          <div className="grow">
            <ProgressBar value={progressWidth({ ...c, state: c.jobState })} />
            <div className={cn(note, "mt-1")}>{formatProgress({ ...c, state: c.jobState })}</div>
          </div>
          {phase === "running" && (
            <Button type="button" variant="ghost" onClick={() => void cancel(c.key)}>
              Cancel
            </Button>
          )}
          <ActionError c={c} />
        </div>
      );
    case "done":
      return (
        <Actions>
          {c.jobId && (
            <Button asChild variant="done">
              <a href={fileUrl(c.jobId)} download={c.filename ?? ""}>
                Save
              </a>
            </Button>
          )}
          <span className="text-[11px] text-[#2d7a46]">{doneNote(c)}</span>
          <Button type="button" variant="ghost" onClick={() => void remove(c.key)}>
            Remove
          </Button>
          <ActionError c={c} />
        </Actions>
      );
    case "stopped":
      return (
        <Actions>
          {c.resumable ? (
            <Button type="button" onClick={() => void resume(c.key)}>
              Resume
            </Button>
          ) : (
            canRetry(c) && (
              <Button type="button" onClick={() => void download(c.key)}>
                Retry
              </Button>
            )
          )}
          {c.jobId && (
            <Button type="button" variant="ghost" onClick={() => void remove(c.key)}>
              Remove
            </Button>
          )}
          <span className={c.jobState === "cancelled" ? note : noteError}>{stoppedNote(c)}</span>
          <ActionError c={c} />
        </Actions>
      );
    default:
      return null;
  }
}
