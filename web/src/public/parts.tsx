import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { cn } from "@/lib/utils";
import { MusicIcon } from "./icons";

const PHONE = "(max-width: 560px)";

/** True on phone widths (≤ 560 px), which get their own layout (MainMobile mockup). */
export function usePhone(): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const mq = window.matchMedia(PHONE);
      mq.addEventListener("change", onChange);
      return () => mq.removeEventListener("change", onChange);
    },
    () => window.matchMedia(PHONE).matches,
  );
}

/** Re-render every second while `active` (countdowns). */
export function useTick(active: boolean): void {
  const [, setN] = useState(0);
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => setN((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [active]);
}

/** Media thumbnail, or the plain placeholder box of the mockups. */
export function Thumb({ src, audio, className }: { src?: string | null; audio?: boolean; className?: string }) {
  const [broken, setBroken] = useState(false);
  useEffect(() => setBroken(false), [src]);
  return (
    <div className={cn("flex shrink-0 items-center justify-center overflow-hidden bg-thumb text-muted", className)}>
      {src && !broken ? (
        <img src={src} alt="" referrerPolicy="no-referrer" className="size-full object-cover" onError={() => setBroken(true)} />
      ) : audio ? (
        <MusicIcon />
      ) : null}
    </div>
  );
}

/** Look: .rc-bar. The fill width is set through the CSSOM (the CSP forbids style attributes). */
export function ProgressBar({ value }: { value: number }) {
  const fill = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    if (fill.current) fill.current.style.width = `${value}%`;
  }, [value]);
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value)}
      className="h-1.5 overflow-hidden rounded-[3px] bg-line-soft"
    >
      <span ref={fill} data-progress-fill className="block h-full w-0 bg-accent transition-[width] duration-300" />
    </div>
  );
}

/** Quality chips (.rc-chip): one pressed at a time. */
export function Chips({
  options,
  value,
  onPick,
  className,
  chipClassName,
}: {
  options: { id: string; label: string }[];
  value?: string;
  onPick: (id: string) => void;
  className?: string;
  chipClassName?: string;
}) {
  return (
    <div role="group" aria-label="Quality" className={className}>
      {options.map((f) => (
        <button
          key={f.id}
          type="button"
          aria-pressed={f.id === value}
          onClick={() => onPick(f.id)}
          className={cn(
            "cursor-pointer rounded-md border-[1.5px] border-line bg-transparent px-2.5 py-1 font-mono text-[11px] text-muted",
            "aria-pressed:border-fg aria-pressed:bg-[rgba(26,26,24,0.04)] aria-pressed:text-fg",
            chipClassName,
          )}
        >
          {f.label}
        </button>
      ))}
    </div>
  );
}
