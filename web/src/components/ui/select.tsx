import { Select as SelectPrimitive } from "radix-ui";
import { Check, ChevronDown } from "lucide-react";
import type { ComponentProps } from "react";
import { styleNonce } from "@/lib/nonce";
import { cn } from "@/lib/utils";

/* Looks: .rc-trigger (+ .wide) and .rc-menu / .rc-menu li */
const Select = SelectPrimitive.Root;
const SelectGroup = SelectPrimitive.Group;
const SelectValue = SelectPrimitive.Value;

function SelectTrigger({
  className,
  children,
  wide = false,
  ...props
}: ComponentProps<typeof SelectPrimitive.Trigger> & { wide?: boolean }) {
  return (
    <SelectPrimitive.Trigger
      data-slot="select-trigger"
      className={cn(
        "inline-flex cursor-pointer items-center justify-between gap-2 border-[1.5px] border-line bg-card text-fg outline-none",
        "data-[state=open]:border-accent data-[state=open]:shadow-focus focus-visible:border-accent focus-visible:shadow-focus",
        "data-[placeholder]:text-muted disabled:cursor-not-allowed disabled:opacity-50",
        wide ? "w-full rounded-[10px] px-3 py-[11px] text-[13px]" : "rounded-lg px-3 py-[7px] text-xs",
        className,
      )}
      {...props}
    >
      {children}
      <SelectPrimitive.Icon asChild>
        <ChevronDown className="size-3.5 text-muted" />
      </SelectPrimitive.Icon>
    </SelectPrimitive.Trigger>
  );
}

function SelectContent({
  className,
  children,
  position = "popper",
  ...props
}: ComponentProps<typeof SelectPrimitive.Content>) {
  return (
    <SelectPrimitive.Portal>
      <SelectPrimitive.Content
        data-slot="select-content"
        position={position}
        sideOffset={6}
        className={cn(
          "z-50 min-w-[var(--radix-select-trigger-width)] overflow-hidden rounded-[10px] border-[1.5px] border-line bg-card font-mono text-fg shadow-menu",
          className,
        )}
        {...props}
      >
        <SelectPrimitive.Viewport nonce={styleNonce()} className="max-h-[var(--radix-select-content-available-height)] p-1.5">
          {children}
        </SelectPrimitive.Viewport>
      </SelectPrimitive.Content>
    </SelectPrimitive.Portal>
  );
}

function SelectItem({ className, children, ...props }: ComponentProps<typeof SelectPrimitive.Item>) {
  return (
    <SelectPrimitive.Item
      data-slot="select-item"
      className={cn(
        "relative flex cursor-pointer select-none items-center gap-2 rounded-md py-2 pr-2.5 pl-8 text-xs text-fg outline-none",
        "data-[highlighted]:bg-line-soft data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
        className,
      )}
      {...props}
    >
      <span className="absolute left-2.5 flex size-3.5 items-center justify-center text-accent">
        <SelectPrimitive.ItemIndicator>
          <Check className="size-3.5" />
        </SelectPrimitive.ItemIndicator>
      </span>
      <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
    </SelectPrimitive.Item>
  );
}

function SelectSeparator({ className, ...props }: ComponentProps<typeof SelectPrimitive.Separator>) {
  return <SelectPrimitive.Separator className={cn("my-1 h-px bg-line-soft", className)} {...props} />;
}

export { Select, SelectGroup, SelectValue, SelectTrigger, SelectContent, SelectItem, SelectSeparator };
