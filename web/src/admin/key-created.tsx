import { Check, Copy } from "lucide-react";
import { useRef, useState } from "react";
import type { CreatedKey } from "@/api/types.gen";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import type { Layout } from "./breakpoint";
import { keyPrefix } from "./lib";
import { Modal, ModalDescription, ModalTitle } from "./modal";

const CHECK_BADGE = "inline-flex shrink-0 items-center justify-center rounded-full bg-success-soft text-success-ink";

export function KeyCreatedDialog({ layout, created, onClose }: { layout: Layout; created: CreatedKey | null; onClose: () => void }) {
  const phone = layout === "phone";
  const [copied, setCopied] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const key = created?.key ?? "";
  const name = created?.info.name ?? "";
  const snippet = `curl -H "Authorization: Bearer ${created ? keyPrefix(created.info.id) : ""}" \\\n  ${location.origin}/api/v1/jobs`;

  async function copy() {
    try {
      await navigator.clipboard.writeText(key);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      input.current?.select(); // clipboard blocked: leave the key selected for a manual copy
    }
  }

  const close = () => {
    setCopied(false);
    onClose();
  };
  const copyLabel = copied ? "Copied" : phone ? "Copy key" : "Copy";
  const CopyIcon = copied ? Check : Copy;

  return (
    <Modal layout={layout} open={created !== null} onOpenChange={(o) => !o && close()} locked className={phone ? "gap-5" : "max-w-[600px]"}>
      {phone ? (
        <div className="flex flex-col items-center gap-3 text-center">
          <span className={cn(CHECK_BADGE, "size-12")}>
            <Check className="size-[22px]" strokeWidth={2.2} aria-hidden />
          </span>
          <ModalTitle className="text-[26px] leading-[1.15]">Key created</ModalTitle>
          <ModalDescription className="text-xs">
            Copy it now. You will not see it again: ReClip only keeps a fingerprint of it.
          </ModalDescription>
        </div>
      ) : (
        <>
          <div className="flex items-center gap-3">
            <span className={cn(CHECK_BADGE, "size-9")}>
              <Check className="size-[18px]" strokeWidth={2.2} aria-hidden />
            </span>
            <ModalTitle>Key created: {name}</ModalTitle>
          </div>
          <ModalDescription className="text-xs leading-[1.6] text-[#3a3a38]">
            Copy it now and store it somewhere safe. You will not see it again: ReClip only keeps a fingerprint of it.
          </ModalDescription>
        </>
      )}

      {phone ? (
        <div className="flex flex-col gap-2">
          <span className="text-[11px] tracking-[0.04em] text-muted uppercase">{name}</span>
          <p className="m-0 rounded-[10px] border-[1.5px] border-dashed border-line bg-field p-3.5 text-[13px] leading-[1.6] break-all">{key}</p>
          <Button type="button" size="cta" className="h-12 text-xs" onClick={copy}>
            <CopyIcon className="size-4" aria-hidden />
            {copyLabel}
          </Button>
        </div>
      ) : (
        <div className="flex gap-2">
          <Label htmlFor="new-key-value" className="sr-only">
            API key
          </Label>
          <Input id="new-key-value" ref={input} readOnly value={key} className="min-w-0 flex-1 bg-field text-xs" onFocus={(e) => e.currentTarget.select()} />
          <Button type="button" size="md" className="rounded-[10px] px-4 py-0 tracking-[0.04em]" onClick={copy}>
            <CopyIcon className="size-3.5" aria-hidden />
            {copyLabel}
          </Button>
        </div>
      )}

      <div>
        <p className="mt-0 mb-2 text-[11px] tracking-[0.04em] text-muted uppercase">Use it</p>
        <pre className="m-0 rounded-[10px] border-[1.5px] border-line bg-code p-3.5 text-[11px] leading-[1.6] break-all whitespace-pre-wrap">{snippet}</pre>
      </div>
      <div className={phone ? undefined : "flex flex-wrap justify-end gap-2.5"}>
        <Button type="button" variant="primary" size={phone ? "cta" : "md"} onClick={close}>
          I copied it
        </Button>
      </div>
    </Modal>
  );
}
