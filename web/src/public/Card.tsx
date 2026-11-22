import { type ReactNode, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { fileUrl } from "../api/client";
import type { MediaFormat } from "../api/types.gen";
import { AUDIO_FORMATS, type Card, FORMAT_LABELS, VIDEO_FORMATS, canRetry, cardFormat, h264First, isVideo, sortedFormats } from "../state";
import { cardMeta, cardTitle, doneNote, phaseOf, stoppedNote, waitLeft } from "./describe";
import { cancel, download, pickCardFormat, pickFormat, remove, resume } from "./downloads";
import { type FormatGroup, FormatMenu, type QualityColumn } from "./FormatMenu";
import { formatProgress, progressWidth } from "./format";
import { ChevronIcon, ClockIcon, InfoIcon } from "./icons";
import { ProgressBar, Thumb, useTick } from "./parts";
import { usePage } from "./store";

export const note = "text-[11px] text-muted";
export const noteError = "text-[11px] text-error";

/** Quality chips are offered for video downloads with more than one quality. */
/** The format this card downloads in (its own pick, else the page's). */
export const useCardFormat = (c: Card): MediaFormat => usePage((s) => cardFormat(c, s.options));

export function useQualities(c: Card, phone = false) {
  const format = useCardFormat(c);
  const formats = c.info?.formats ?? [];
  if (!isVideo(format) || formats.length < 2) return [];
  return phone ? h264First(formats) : sortedFormats(formats);
}

/** "Download MP4 1080p", "Download FLAC". */
export function useDownloadLabel(c: Card, quality?: string): string {
  const format = useCardFormat(c);
  return [`Download ${FORMAT_LABELS[format]}`, isVideo(format) && quality].filter(Boolean).join(" ");
}

/** A started card shows its job's format; a ready one the format it will take. */
export const isAudioCard = (c: Card, next: MediaFormat): boolean => !isVideo(c.format ?? next);

/**
 * Download button with a menu that sets this card's own format, so one batch
 * can mix an MP4 video and an MP3 track. A music card lists audio formats only.
 */
export function DownloadButton({
  c,
  label,
  className,
  menuClassName,
  full,
  quality,
}: {
  c: Card;
  label: string;
  className?: string;
  menuClassName?: string;
  full?: boolean;
  quality?: QualityColumn;
}) {
  const format = useCardFormat(c);
  const groups: FormatGroup[] = c.info?.audio_only
    ? [{ formats: AUDIO_FORMATS }]
    : [
        { label: "Video", formats: VIDEO_FORMATS },
        { label: "Audio", formats: AUDIO_FORMATS },
      ];
  return (
    <div className={full ? "flex w-full" : "inline-flex"}>
      <Button type="button" className={cn(className, "rounded-r-none")} onClick={() => void download(c.key)}>
        {label}
      </Button>
      <FormatMenu
        label="Format of this download"
        groups={groups}
        value={format}
        onPick={(f) => pickCardFormat(c.key, f)}
        quality={quality}
        trigger={
          <Button type="button" aria-label="Change the format of this download" className={cn("border-l border-bg/25 px-2", menuClassName, "rounded-l-none")}>
            <ChevronIcon size={10} />
          </Button>
        }
      />
    </div>
  );
}

/** Cancel / Resume / Remove failures, under the actions. */
export const ActionError = ({ c }: { c: Card }) => (c.actionError ? <span className={noteError}>{c.actionError}</span> : null);

/** One link on desktop and tablet (.rc-card). */
export function CardView({ c }: { c: Card }) {
  const phase = phaseOf(c);
  const format = useCardFormat(c);
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
      <Thumb
        src={c.thumbnail ?? c.info?.thumbnail}
        audio={isAudioCard(c, format)}
        className={cn("h-20 rounded-lg", c.info?.audio_only ? "w-20" : "w-[120px]")}
      />
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
            <MetaLine c={c} text={cardMeta(c)} className="mb-2" />
            <Body c={c} />
          </>
        )}
      </div>
    </article>
  );
}

/** Why a music platform's track sounds like YouTube: shown on demand, not in the card. */
function MatchInfo({ from }: { from: string }) {
  const [open, setOpen] = useState(false);
  return (
    <Tooltip open={open} onOpenChange={setOpen}>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={`About ${from} downloads`}
          // A tap toggles it (touch screens have no hover); Radix would close it on click.
          onClick={(e) => {
            e.preventDefault();
            setOpen((o) => !o);
          }}
          // 44 px touch target that takes no room in the 11 px line.
          className="-my-4 -mr-4 -ml-3 flex size-11 shrink-0 cursor-help items-center justify-center text-muted hover:text-fg focus-visible:text-fg focus-visible:outline-none"
        >
          <InfoIcon size={12} />
        </button>
      </TooltipTrigger>
      <TooltipContent side="top" collisionPadding={12} className="max-w-[264px] normal-case tracking-normal">
        {from} encrypts its audio: the file is the matching YouTube upload (about 130 kbps), tagged with {from}'s title, artist and cover.
      </TooltipContent>
    </Tooltip>
  );
}

/** The uppercase meta line, with the music source's info icon. */
export function MetaLine({ c, text, className }: { c: Card; text: string; className?: string }) {
  const from = c.info?.matched_from;
  return (
    <div className={cn("flex min-w-0 items-center gap-1.5 text-[11px] tracking-[0.03em] text-muted uppercase", className)}>
      <span className="truncate">{text}</span>
      {from && <MatchInfo from={from} />}
    </div>
  );
}

const Actions = ({ children }: { children: ReactNode }) => <div className="flex flex-wrap items-center gap-1.5">{children}</div>;

function Body({ c }: { c: Card }) {
  const format = useCardFormat(c);
  const all = c.info?.formats ?? [];
  // Desktop: the quality sits in the format menu, so the actions never wrap.
  const quality: QualityColumn | undefined =
    all.length > 1
      ? { options: sortedFormats(all), value: c.selectedFormat, onPick: (id) => pickFormat(c.key, id), disabled: !isVideo(format) }
      : undefined;
  const label = useDownloadLabel(c, all.find((f) => f.id === c.selectedFormat)?.label);
  const phase = phaseOf(c);
  switch (phase) {
    case "ready":
      return (
        <Actions>
          <DownloadButton c={c} label={label} quality={quality} />
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
