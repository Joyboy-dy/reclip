import { Slot } from "radix-ui";
import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/* Looks: .rc-primary / .rc-btn / .rc-btn.ghost / .rc-secondary / .rc-danger / .rc-btn.done / .rc-icon-btn / .m-cta / .m-btn */
const buttonVariants = cva(
  "inline-flex cursor-pointer items-center justify-center gap-2 text-center uppercase no-underline transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "rounded-[10px] bg-accent font-medium tracking-[0.04em] text-white hover:bg-accent-hover hover:text-white",
        default: "rounded-[7px] bg-fg font-medium tracking-[0.03em] text-bg hover:bg-fg/90 hover:text-bg",
        ghost: "rounded-[7px] border-[1.5px] border-line bg-transparent font-normal tracking-[0.03em] text-fg hover:bg-line-soft hover:text-fg",
        secondary: "rounded-[10px] border-[1.5px] border-line bg-transparent font-normal tracking-[0.04em] text-fg hover:bg-line-soft hover:text-fg",
        danger: "rounded-[10px] bg-error font-medium tracking-[0.04em] text-white hover:bg-error-ink hover:text-white",
        done: "rounded-[7px] bg-success font-medium tracking-[0.03em] text-white hover:bg-success hover:text-white",
        icon: "rounded-[7px] bg-transparent normal-case text-muted hover:bg-line-soft hover:text-fg",
      },
      size: {
        default: "px-[14px] py-1.5 text-[11px]",
        md: "px-[18px] py-[11px] text-xs",
        lg: "px-5 py-[11px] text-[13px]",
        touch: "h-10 px-4 text-xs",
        cta: "h-12 w-full text-[13px] tracking-[0.05em]",
        icon: "size-8 p-0",
        "icon-touch": "size-10 p-0",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);

type ButtonProps = ComponentProps<"button"> & VariantProps<typeof buttonVariants> & { asChild?: boolean };

function Button({ className, variant, size, asChild = false, ...props }: ButtonProps) {
  const Comp = asChild ? Slot.Root : "button";
  return <Comp data-slot="button" className={cn(buttonVariants({ variant, size }), className)} {...props} />;
}

export { Button, buttonVariants };
