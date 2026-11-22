import type { ReactNode } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { FormatOption, MediaFormat } from "../api/types.gen";
import { FORMAT_HINTS, FORMAT_LABELS } from "../state";

export type FormatGroup = { label?: string; formats: readonly MediaFormat[] };

/** The video qualities of a card, shown as a second column. */
export type QualityColumn = { options: FormatOption[]; value?: string; onPick: (id: string) => void; disabled: boolean };

const heading = "tracking-[0.04em] uppercase";

/** A custom (never native) single-choice format list, opened by `trigger`; with `quality`, a Quality column beside it. */
export function FormatMenu({
  trigger,
  groups,
  value,
  onPick,
  label,
  quality,
}: {
  trigger: ReactNode;
  groups: FormatGroup[];
  value?: MediaFormat;
  onPick: (f: MediaFormat) => void;
  label: string;
  quality?: QualityColumn;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{trigger}</DropdownMenuTrigger>
      <DropdownMenuContent aria-label={label} collisionPadding={12} align="start" className="flex min-w-0 gap-1.5">
        <DropdownMenuRadioGroup className="w-[244px]" value={value ?? ""} onValueChange={(v) => onPick(v as MediaFormat)}>
          {groups.map((g, i) => (
            <div key={g.label ?? i}>
              {i > 0 && <DropdownMenuSeparator />}
              {g.label && <DropdownMenuLabel className={heading}>{g.label}</DropdownMenuLabel>}
              {g.formats.map((f) => (
                <DropdownMenuRadioItem key={f} value={f} className="min-h-8 py-1.5">
                  <span className="w-[68px] shrink-0">{FORMAT_LABELS[f]}</span>
                  <span className="truncate text-[11px] text-muted">{FORMAT_HINTS[f]}</span>
                </DropdownMenuRadioItem>
              ))}
            </div>
          ))}
        </DropdownMenuRadioGroup>
        {quality && (
          <DropdownMenuRadioGroup
            className="w-[128px] border-l border-line-soft pl-1.5"
            value={quality.value ?? ""}
            onValueChange={quality.onPick}
          >
            <DropdownMenuLabel className={heading}>Quality</DropdownMenuLabel>
            {quality.options.map((q) => (
              <DropdownMenuRadioItem key={q.id} value={q.id} disabled={quality.disabled} className="min-h-8 py-1.5">
                {q.label}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
