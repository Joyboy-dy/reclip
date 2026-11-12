import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/* Looks: .rc-input (12/14 padding, 10px radius) and the phone .m-input (48px) */
function Input({ className, touch = false, ...props }: ComponentProps<"input"> & { touch?: boolean }) {
  return (
    <input
      data-slot="input"
      className={cn(
        "w-full rounded-[10px] border-[1.5px] border-line bg-card font-mono text-fg outline-none placeholder:text-muted/70",
        "focus:border-accent focus:shadow-focus disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-error",
        touch ? "h-12 px-3.5 text-sm" : "px-3.5 py-3 text-[13px]",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
