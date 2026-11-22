import { AlertDialog } from "radix-ui";
import { useEffect, useState } from "react";
import type { BlockKind } from "@/api/types.gen";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { adminErrorText, createBlock, isUnauthorized } from "./api";
import type { Layout } from "./breakpoint";
import { ErrorLine, Field, TextInput } from "./parts";

/** What to block: a known client or address, or (no `target`) an address typed in. */
export type BlockAsk = { kind: BlockKind; target?: string; name?: string };

export function BlockDialog({
  layout,
  ask,
  onClose,
  onBlocked,
  onUnauthorized,
}: {
  layout: Layout;
  ask: BlockAsk | null;
  onClose: () => void;
  onBlocked: () => void;
  onUnauthorized: () => void;
}) {
  const phone = layout === "phone";
  const typed = ask !== null && ask.target === undefined;
  const [address, setAddress] = useState("");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (ask) {
      setAddress("");
      setReason("");
      setError(null);
    }
  }, [ask]);

  async function block() {
    if (!ask) return;
    const target = ask.target ?? address.trim();
    if (!target) return setError("Enter an address or a range.");
    setBusy(true);
    setError(null);
    try {
      await createBlock({ kind: ask.kind, target, reason: reason.trim() });
      onBlocked();
    } catch (err) {
      if (isUnauthorized(err)) return onUnauthorized();
      setError(adminErrorText(err));
    } finally {
      setBusy(false);
    }
  }

  const what = ask?.kind === "ip" ? "address" : "user";
  const title = typed ? "Block an address" : `Block ${ask?.name ?? "this " + what}?`;
  const effect =
    ask?.kind === "ip"
      ? "New links and downloads from this address (or range) are refused, whatever the browser or key. Downloads already started finish normally."
      : "New links and downloads from this user are refused with a clear message, on the site and in the bot. Downloads already started finish normally.";

  return (
    <AlertDialog.Root open={ask !== null} onOpenChange={(o) => !o && onClose()}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="fixed inset-0 z-50 bg-scrim/85" />
        <AlertDialog.Content
          className={cn(
            "fixed z-50 flex flex-col gap-4 bg-card font-mono text-fg outline-none",
            phone
              ? "inset-x-0 bottom-0 max-h-[90vh] overflow-y-auto rounded-t-[20px] px-4 pt-2 pb-6 shadow-sheet"
              : "top-1/2 left-1/2 w-[calc(100%-48px)] max-w-[480px] -translate-x-1/2 -translate-y-1/2 rounded-2xl p-7 shadow-dialog",
          )}
        >
          {phone && <div aria-hidden className="h-1 w-10 shrink-0 self-center rounded-sm bg-line" />}
          <AlertDialog.Title className={cn("m-0 font-serif font-normal break-words", phone ? "text-[26px] leading-[1.15]" : "text-[30px] leading-[1.15]")}>
            {title}
          </AlertDialog.Title>
          <AlertDialog.Description asChild>
            <p className={cn("m-0", phone ? "text-[13px] leading-[1.6]" : "text-xs leading-[1.7]")}>{effect}</p>
          </AlertDialog.Description>
          {typed && (
            <Field id="block-address" label="Address or range">
              <TextInput
                id="block-address"
                value={address}
                autoComplete="off"
                spellCheck={false}
                placeholder="203.0.113.7 or 203.0.113.0/24"
                onChange={(e) => setAddress(e.target.value)}
              />
            </Field>
          )}
          <Field id="block-reason" label="Reason (only admins see it)">
            <TextInput
              id="block-reason"
              value={reason}
              maxLength={200}
              placeholder="Shared the link on a forum"
              onChange={(e) => setReason(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void block();
              }}
            />
          </Field>
          <p className="m-0 text-xs leading-[1.6] text-muted">You can lift the block at any time.</p>
          {error && <ErrorLine>{error}</ErrorLine>}
          <div className={cn("mt-1.5", phone ? "mt-2 grid grid-cols-2 gap-2" : "flex flex-wrap justify-end gap-2.5")}>
            <AlertDialog.Cancel asChild>
              <Button type="button" variant="secondary" size="md" className={phone ? "h-12" : undefined}>
                Cancel
              </Button>
            </AlertDialog.Cancel>
            <Button type="button" variant="danger" size="md" className={phone ? "h-12" : undefined} disabled={busy} onClick={() => void block()}>
              Block
            </Button>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
