import { CircleAlert } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { passwordStrength } from "./lib";

/** Text input: 12px-padded on desktop/tablet, 48 px tall on phones. */
export function TextInput({ className, ...props }: ComponentProps<typeof Input>) {
  return <Input className={cn("max-[560px]:h-12 max-[560px]:py-0 max-[560px]:text-sm", className)} {...props} />;
}

export function Field({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5 max-[560px]:gap-2">
      <Label htmlFor={id}>{label}</Label>
      {children}
    </div>
  );
}

/** Checkbox + label. Desktop: box then text. Phone: text left, box right, 44 px row. */
export function CheckRow({
  id,
  checked,
  onChange,
  children,
  className,
}: {
  id: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex items-center gap-2 max-[560px]:min-h-11 max-[560px]:flex-row-reverse max-[560px]:justify-between", className)}>
      <Checkbox id={id} checked={checked} onCheckedChange={(v) => onChange(v === true)} />
      <Label htmlFor={id} className="cursor-pointer text-xs tracking-normal text-fg normal-case max-[560px]:text-[13px]">
        {children}
      </Label>
    </div>
  );
}

export function ErrorLine({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="m-0 flex items-center gap-1.5 text-[11px] text-error">
      <CircleAlert className="size-3.5 shrink-0" aria-hidden />
      {children}
    </p>
  );
}

/** Centered form page: vertically centered on larger screens, top-aligned on phones. */
export function CenteredPage({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-screen items-center justify-center px-6 py-10 max-[560px]:items-start max-[560px]:px-4 max-[560px]:pt-14 max-[560px]:pb-8">
      {children}
    </div>
  );
}

export function Panel({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      className={cn(
        "flex flex-col gap-3.5 rounded-[14px] border-[1.5px] border-line bg-card p-6 max-[560px]:gap-4 max-[560px]:px-4 max-[560px]:py-5",
        className,
      )}
      {...props}
    />
  );
}

export function PageTitle({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div>
      <h1 className="m-0 font-serif text-[36px] font-normal tracking-[-0.02em] max-[560px]:text-[32px] max-[560px]:leading-[1.1]">
        {title}
      </h1>
      <p className="mt-1.5 mb-0 text-xs leading-[1.6] text-muted">{children}</p>
    </div>
  );
}

/** Four-bar strength meter plus the one-line verdict under a new-password field. */
export function StrengthMeter({ password }: { password: string }) {
  const { bars, label } = passwordStrength(password);
  return (
    <>
      <div className="mt-1 grid grid-cols-4 gap-1" aria-hidden>
        {[1, 2, 3, 4].map((n) => (
          <div key={n} className={cn("h-1 rounded-sm", n <= bars ? "bg-success" : "bg-line-soft")} />
        ))}
      </div>
      <p className={cn("m-0 text-[11px]", bars >= 3 ? "text-[#2d7a46]" : "text-muted")} aria-live="polite">
        {label}
      </p>
    </>
  );
}
