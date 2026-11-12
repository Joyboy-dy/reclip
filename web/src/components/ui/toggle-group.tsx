import { ToggleGroup as ToggleGroupPrimitive } from "radix-ui";
import { createContext, useContext, type ComponentProps } from "react";
import { cn } from "@/lib/utils";

/* Looks: .rc-seg (+ .soft) and the phone .m-seg (44px cells, equal columns) */
type Tone = "solid" | "soft";
type Density = "default" | "touch";
const Ctx = createContext<{ tone: Tone; density: Density }>({ tone: "solid", density: "default" });

type RootProps = (ComponentProps<typeof ToggleGroupPrimitive.Root>) & { tone?: Tone; density?: Density };

function ToggleGroup({ className, tone = "solid", density = "default", ...props }: RootProps) {
  return (
    <Ctx.Provider value={{ tone, density }}>
      <ToggleGroupPrimitive.Root
        data-slot="toggle-group"
        className={cn(
          "overflow-hidden rounded-[10px] border-[1.5px] border-line bg-card",
          density === "touch" ? "grid grid-flow-col auto-cols-fr" : "flex",
          className,
        )}
        {...props}
      />
    </Ctx.Provider>
  );
}

function ToggleGroupItem({ className, ...props }: ComponentProps<typeof ToggleGroupPrimitive.Item>) {
  const { tone, density } = useContext(Ctx);
  return (
    <ToggleGroupPrimitive.Item
      data-slot="toggle-group-item"
      className={cn(
        "cursor-pointer bg-transparent text-xs uppercase text-muted outline-none focus-visible:shadow-[inset_0_0_0_2px_var(--color-accent)]",
        density === "touch"
          ? "h-11 border-r border-line-soft tracking-[0.04em] last:border-r-0"
          : "px-4 py-[9px] tracking-[0.03em]",
        tone === "soft"
          ? "data-[state=on]:bg-accent-soft data-[state=on]:text-accent-ink"
          : "data-[state=on]:bg-fg data-[state=on]:text-bg",
        className,
      )}
      {...props}
    />
  );
}

export { ToggleGroup, ToggleGroupItem };
