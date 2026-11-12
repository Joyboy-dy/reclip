import { Label as LabelPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/* Look: .rc-label */
function Label({ className, ...props }: ComponentProps<typeof LabelPrimitive.Root>) {
  return (
    <LabelPrimitive.Root
      data-slot="label"
      className={cn("text-[11px] tracking-[0.04em] text-muted uppercase", className)}
      {...props}
    />
  );
}

export { Label };
