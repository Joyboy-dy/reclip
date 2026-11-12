import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/* Look: .rc-badge (+ .off) */
const badgeVariants = cva("inline-block whitespace-nowrap rounded-md px-2 py-[3px] text-[11px]", {
  variants: {
    variant: {
      success: "bg-success-soft text-success-ink",
      off: "bg-error-soft text-error-ink",
      accent: "bg-accent-soft text-accent-ink",
    },
  },
  defaultVariants: { variant: "success" },
});

function Badge({ className, variant, ...props }: ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return <span data-slot="badge" className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
