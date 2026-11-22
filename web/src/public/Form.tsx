import { useLayoutEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { AUDIO_LABELS, AUDIO_MAIN, AUDIO_MORE, type AudioFormat, VIDEO_FORMATS, type VideoFormat } from "../state";
import { loadDraft, saveDraft } from "../storage";
import { parseUrls } from "../urls";
import { fetchLinks } from "./downloads";
import { clipFromInput } from "./format";
import { FormatMenu } from "./FormatMenu";
import { ChevronIcon } from "./icons";
import { type Media, setOptions, usePage } from "./store";


/** The link box: Enter adds a line, Ctrl+Enter (⌘+Enter) fetches; `submit` is shared with the Fetch button. */
export function useLinks() {
  const [text, setText] = useState(loadDraft);
  const busy = usePage((s) => s.busy);
  const submit = async () => {
    if (busy) return;
    if (await fetchLinks(parseUrls(text))) saveDraft("");
  };
  const change = (value: string) => {
    setText(value);
    saveDraft(value);
  };
  return { text, change, submit, busy };
}

export type Links = ReturnType<typeof useLinks>;

/** Lines the link box grows to before it scrolls. */
const MAX_LINES = 8;

/** Fit the box to its links (at least `rows`, at most MAX_LINES), through the CSSOM: the CSP forbids style attributes. */
function useAutoHeight(text: string) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const cs = getComputedStyle(el);
    const chrome = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom) + el.offsetHeight - el.clientHeight;
    const max = parseFloat(cs.lineHeight) * MAX_LINES + chrome;
    el.style.height = "auto";
    const want = el.scrollHeight + el.offsetHeight - el.clientHeight;
    el.style.height = `${Math.min(want, max)}px`;
    el.style.overflowY = want > max ? "auto" : "hidden";
  }, [text]);
  return ref;
}

export function LinkBox({ links, phone }: { links: Links; phone?: boolean }) {
  const ref = useAutoHeight(links.text);
  return (
    <>
      <label htmlFor="urls" className="sr-only">
        Links
      </label>
      <textarea
        ref={ref}
        id="urls"
        rows={phone ? 3 : 2}
        placeholder="Paste one or more links..."
        value={links.text}
        onChange={(e) => links.change(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            void links.submit();
          }
        }}
        className={cn(
          "block w-full resize-none rounded-card border-[1.5px] border-line bg-card font-mono text-fg outline-none placeholder:text-muted/70 focus:border-accent focus:shadow-focus",
          // px line heights: the auto height counts lines.
          phone ? "m-0 px-4 py-3.5 text-[13px] leading-[19px]" : "mb-1.5 px-5 py-[18px] text-sm leading-5",
        )}
      />
    </>
  );
}

export function FetchButton({ links, phone }: { links: Links; phone?: boolean }) {
  return (
    <Button
      type="button"
      variant="primary"
      size={phone ? "cta" : "lg"}
      disabled={links.busy}
      onClick={() => void links.submit()}
      // Desktop: as tall as the segments beside it.
      className={phone ? undefined : "h-9 min-w-0 flex-1 py-0"}
    >
      {links.busy ? "Loading..." : "Fetch"}
    </Button>
  );
}

/** Video/Audio, then three format segments (.rc-seg / .m-seg); the audio ones end with "More", a menu of the rest. */
export function MediaControls({ phone }: { phone?: boolean }) {
  const { media, container, audio } = usePage((s) => s.options);
  const density = phone ? "touch" : "default";
  // Fixed 96 px cells on desktop: both modes are 291 px wide, so Fetch never moves.
  const cell = phone ? undefined : "w-24 px-0";
  const inMore = (AUDIO_MORE as readonly string[]).includes(audio);
  return (
    <>
      <ToggleGroup
        type="single"
        aria-label="Media"
        density={density}
        value={media}
        onValueChange={(v) => v && setOptions({ media: v as Media })}
      >
        <ToggleGroupItem value="video">Video</ToggleGroupItem>
        <ToggleGroupItem value="audio">Audio</ToggleGroupItem>
      </ToggleGroup>
      {media === "video" ? (
        <ToggleGroup
          type="single"
          aria-label="Container"
          tone="soft"
          density={density}
          value={container}
          onValueChange={(v) => v && setOptions({ container: v as VideoFormat })}
        >
          {VIDEO_FORMATS.map((f) => (
            <ToggleGroupItem key={f} value={f} className={cell}>
              {f.toUpperCase()}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      ) : (
        <div className={cn("overflow-hidden rounded-[10px] border-[1.5px] border-line bg-card", phone ? "grid grid-cols-3" : "flex")}>
          <ToggleGroup
            type="single"
            aria-label="Audio format"
            tone="soft"
            density={density}
            className={cn("rounded-none border-0", phone && "col-span-2")}
            value={inMore ? "" : audio}
            onValueChange={(v) => v && setOptions({ audio: v as AudioFormat })}
          >
            {AUDIO_MAIN.map((f) => (
              <ToggleGroupItem key={f} value={f} className={cell}>
                {AUDIO_LABELS[f]}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <FormatMenu
            label="More audio formats"
            groups={[{ formats: AUDIO_MORE }]}
            value={inMore ? audio : undefined}
            onPick={(f) => setOptions({ audio: f as AudioFormat })}
            trigger={
              <button
                type="button"
                aria-label={inMore ? `Audio format ${AUDIO_LABELS[audio]}, more formats` : "More audio formats"}
                className={cn(
                  "inline-flex cursor-pointer items-center justify-center gap-1 text-xs uppercase outline-none focus-visible:shadow-[inset_0_0_0_2px_var(--color-accent)]",
                  phone ? "h-11 border-l border-line-soft tracking-[0.04em]" : "w-24 py-[9px] tracking-[0.03em]",
                  inMore ? "bg-accent-soft text-accent-ink" : "text-muted",
                )}
              >
                {inMore ? AUDIO_LABELS[audio] : "More"}
                <ChevronIcon size={10} />
              </button>
            }
          />
        </div>
      )}
    </>
  );
}

const timeInput =
  "rounded-[7px] border-[1.5px] border-line bg-card text-center font-mono text-xs text-fg outline-none focus:border-accent disabled:opacity-50 aria-invalid:border-error";

/** A labelled shadcn Checkbox (.rc-check). */
function Check({ id, label, checked, onChange, phone }: { id: string; label: string; checked: boolean; onChange: (v: boolean) => void; phone?: boolean }) {
  return (
    <span className={cn("inline-flex items-center gap-2 text-fg", phone ? "text-[13px]" : "text-xs")}>
      <Checkbox id={id} checked={checked} onCheckedChange={(v) => onChange(v === true)} />
      {/* On a phone the label is the 44 px touch target of the small box. */}
      <label htmlFor={id} className={cn("cursor-pointer", phone && "inline-flex min-h-11 items-center")}>
        {label}
      </label>
    </span>
  );
}

/** Clip and Remove metadata, applied to the next downloads. */
export function ClipOptions({ phone }: { phone?: boolean }) {
  const o = usePage((s) => s.options);
  const { error } = clipFromInput({ on: o.clipOn, from: o.clipFrom, to: o.clipTo });
  const time = (id: string, label: string, value: string, key: "clipFrom" | "clipTo") => (
    <>
      <label htmlFor={id} className={phone ? "sr-only" : undefined}>
        {label}
      </label>
      <input
        id={id}
        inputMode="numeric"
        placeholder={key === "clipFrom" ? "0:00" : "end"}
        disabled={!o.clipOn}
        aria-invalid={Boolean(error)}
        value={value}
        onChange={(e) => setOptions(key === "clipFrom" ? { clipFrom: e.target.value } : { clipTo: e.target.value })}
        className={cn(timeInput, phone ? "h-11 w-[60px] p-0" : "w-16 px-2 py-1.5")}
      />
    </>
  );
  const clip = <Check id="clip" label="Clip" checked={o.clipOn} onChange={(v) => setOptions({ clipOn: v })} phone={phone} />;
  const strip = <Check id="strip" label="Remove metadata" checked={o.strip} onChange={(v) => setOptions({ strip: v })} phone={phone} />;
  const errorLine = error && <p role="alert" className="m-0 text-[11px] text-error">{error}</p>;

  if (phone) {
    return (
      <>
        <div className="overflow-hidden rounded-card border-[1.5px] border-line bg-card">
          <div className="flex min-h-[52px] items-center justify-between gap-3 border-b border-line-soft px-4 text-[13px]">
            {clip}
            <div className="flex items-center gap-2">
              {time("clip-from", "From", o.clipFrom, "clipFrom")}
              <span aria-hidden="true" className="text-muted">
                to
              </span>
              {time("clip-to", "To", o.clipTo, "clipTo")}
            </div>
          </div>
          <div className="flex min-h-[52px] items-center justify-between gap-3 px-4 text-[13px]">{strip}</div>
        </div>
        {errorLine}
      </>
    );
  }
  return (
    <>
      <div className="mb-2 flex flex-wrap items-center gap-x-[18px] gap-y-2.5 text-xs">
        {clip}
        <div className="flex items-center gap-1.5 text-muted">
          {time("clip-from", "from", o.clipFrom, "clipFrom")}
          {time("clip-to", "to", o.clipTo, "clipTo")}
        </div>
        {strip}
      </div>
      <p className={cn("m-0 mb-[18px] text-[11px] leading-[1.5]", error ? "text-error" : "text-muted")}>
        {error ?? 'Clip and metadata apply to the next downloads. Leave "to" empty to keep the end.'}
      </p>
    </>
  );
}
