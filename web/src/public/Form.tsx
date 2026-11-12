import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { AUDIO_FORMATS, type AudioFormat, VIDEO_FORMATS, type VideoFormat } from "../state";
import { loadDraft, saveDraft } from "../storage";
import { parseUrls } from "../urls";
import { fetchLinks } from "./downloads";
import { clipFromInput } from "./format";
import { type Media, setOptions, usePage } from "./store";

const AUDIO_LABELS: Record<AudioFormat, string> = { mp3: "MP3", m4a: "M4A", opus: "Opus", flac: "FLAC", wav: "WAV" };

/** The link box with its Enter-to-fetch behaviour; `submit` is shared with the Fetch button. */
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

export function LinkBox({ links, phone }: { links: Links; phone?: boolean }) {
  return (
    <>
      <label htmlFor="urls" className="sr-only">
        Links
      </label>
      <textarea
        id="urls"
        rows={phone ? 3 : 2}
        placeholder="Paste one or more links..."
        value={links.text}
        onChange={(e) => links.change(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            void links.submit();
          }
        }}
        className={cn(
          "block w-full resize-none rounded-card border-[1.5px] border-line bg-card font-mono text-fg outline-none placeholder:text-muted/70 focus:border-accent focus:shadow-focus",
          phone ? "m-0 px-4 py-3.5 text-[13px] leading-[normal]" : "mb-1.5 px-5 py-[18px] text-sm leading-[normal]",
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
      className={phone ? undefined : "flex-[1_1_120px]"}
    >
      {links.busy ? "Loading..." : "Fetch"}
    </Button>
  );
}

/** Video/Audio, then the container or the audio format (.rc-seg / .m-seg). */
export function MediaControls({ phone }: { phone?: boolean }) {
  const { media, container, audio } = usePage((s) => s.options);
  const density = phone ? "touch" : "default";
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
            <ToggleGroupItem key={f} value={f}>
              {f.toUpperCase()}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      ) : (
        <ToggleGroup
          type="single"
          aria-label="Audio format"
          tone="soft"
          density={density}
          value={audio}
          onValueChange={(v) => v && setOptions({ audio: v as AudioFormat })}
        >
          {AUDIO_FORMATS.map((f) => (
            <ToggleGroupItem key={f} value={f}>
              {AUDIO_LABELS[f]}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
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
