import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { fileUrl } from "../api/client";
import { type Card, canRetry } from "../state";
import { ActionError, isAudioCard, noteError, useQualities } from "./Card";
import { cardTitle, phaseOf, phoneMeta, stoppedNote, waitLeft } from "./describe";
import { cancel, download, pickFormat, remove, resume } from "./downloads";
import { formatProgress, progressWidth } from "./format";
import { ClockIcon } from "./icons";
import { Chips, ProgressBar, Thumb, useTick } from "./parts";
import { usePage } from "./store";

/** .m-btn, 8 px radius: 44 px, the smallest phone touch target. */
const mBtn = "h-11 rounded-lg px-4 text-xs tracking-[0.04em]";
const status = "flex items-center justify-between gap-3 text-[11px] text-muted";
const Two = ({ children }: { children: ReactNode }) => <div className="grid grid-cols-2 gap-2">{children}</div>;

/** One link on a phone (.m-card): head, then a full-width action row. */
export function PhoneCard({ c }: { c: Card }) {
  const phase = phaseOf(c);
  const media = usePage((s) => s.options.media);
  useTick(phase === "waiting");
  const failed = phase === "info-error";
  return (
    <article data-card className="flex flex-col gap-3 rounded-card border-[1.5px] border-line bg-card p-3" aria-busy={phase === "loading"}>
      <div className="flex items-center gap-3">
        <Thumb
          src={c.thumbnail ?? c.info?.thumbnail}
          audio={isAudioCard(c, media)}
          className={cn("h-16 w-24 rounded-lg", phase === "loading" && "animate-pulse")}
        />
        <div className="min-w-0 flex-1">
          {phase === "loading" ? (
            <>
              <div className="h-3.5 w-4/5 animate-pulse rounded bg-line-soft" />
              <div className="mt-2 h-2.5 w-1/2 animate-pulse rounded bg-line-soft" />
              <span className="sr-only">Fetching {c.url}</span>
            </>
          ) : (
            <>
              <div className={cn("line-clamp-2 font-serif text-[16px] leading-[1.25]", failed && "text-error")}>
                {failed ? "Could not fetch video" : cardTitle(c)}
              </div>
              <div className="mt-1 truncate text-[11px] tracking-[0.03em] text-muted uppercase">{failed ? c.error : phoneMeta(c)}</div>
            </>
          )}
        </div>
      </div>
      <Body c={c} />
    </article>
  );
}

function Body({ c }: { c: Card }) {
  const qualities = useQualities(c, true);
  const phase = phaseOf(c);
  switch (phase) {
    case "ready": {
      const label = qualities.find((f) => f.id === c.selectedFormat)?.label;
      return (
        <>
          {qualities.length > 0 && (
            <Chips
              options={qualities}
              value={c.selectedFormat}
              onPick={(id) => pickFormat(c.key, id)}
              className="-mx-3 flex gap-1.5 overflow-x-auto px-3 [scrollbar-width:none]"
              chipClassName="h-11 flex-none px-3 py-0"
            />
          )}
          <Button type="button" className={mBtn} onClick={() => void download(c.key)}>
            {label ? `Download ${label}` : "Download"}
          </Button>
          {c.error && <span className={noteError}>{c.error}</span>}
        </>
      );
    }
    case "waiting":
      return (
        <div className={status}>
          <span className="inline-flex items-center gap-1.5 text-accent-ink">
            <ClockIcon />
            Starts in {waitLeft(c)} s (rate limit)
          </span>
        </div>
      );
    case "starting":
    case "running":
      return (
        <div className="flex flex-col gap-2">
          <ProgressBar value={progressWidth({ ...c, state: c.jobState })} />
          <div className={status}>
            <span>{formatProgress({ ...c, state: c.jobState })}</span>
            {phase === "running" && (
              <Button type="button" variant="ghost" className="h-11 rounded-lg px-3 text-xs tracking-[0.04em]" onClick={() => void cancel(c.key)}>
                Cancel
              </Button>
            )}
          </div>
          <ActionError c={c} />
        </div>
      );
    case "done":
      return (
        <>
          <Two>
            {c.jobId && (
              <Button asChild variant="done" className={mBtn}>
                <a href={fileUrl(c.jobId)} download={c.filename ?? ""}>
                  Save
                </a>
              </Button>
            )}
            <Button type="button" variant="ghost" className={mBtn} onClick={() => void remove(c.key)}>
              Remove
            </Button>
          </Two>
          <ActionError c={c} />
        </>
      );
    case "stopped": {
      const again = c.resumable ? (
        <Button type="button" className={mBtn} onClick={() => void resume(c.key)}>
          Resume
        </Button>
      ) : (
        canRetry(c) && (
          <Button type="button" className={mBtn} onClick={() => void download(c.key)}>
            Retry
          </Button>
        )
      );
      return (
        <>
          {c.jobId ? (
            <Two>
              {again}
              <Button type="button" variant="ghost" className={mBtn} onClick={() => void remove(c.key)}>
                Remove
              </Button>
            </Two>
          ) : (
            again
          )}
          {c.jobState !== "cancelled" && <span className={noteError}>{stoppedNote(c)}</span>}
          <ActionError c={c} />
        </>
      );
    }
    default:
      return null;
  }
}
