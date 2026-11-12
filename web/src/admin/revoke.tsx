import { AlertDialog } from "radix-ui";
import { useState } from "react";
import type { ApiKeyInfo } from "@/api/types.gen";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { adminErrorText, isUnauthorized, revokeKey } from "./api";
import type { Layout } from "./breakpoint";
import { keyPrefix } from "./lib";
import { ErrorLine } from "./parts";

export function RevokeDialog({
  layout,
  target,
  onClose,
  onRevoked,
  onUnauthorized,
}: {
  onUnauthorized: () => void;
  layout: Layout;
  target: ApiKeyInfo | null;
  onClose: () => void;
  onRevoked: () => void;
}) {
  const phone = layout === "phone";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function revoke() {
    if (!target) return;
    setBusy(true);
    setError(null);
    try {
      await revokeKey(target.id);
      onRevoked();
    } catch (err) {
      if (isUnauthorized(err)) return onUnauthorized();
      setError(adminErrorText(err));
    } finally {
      setBusy(false);
    }
  }

  const close = () => {
    setError(null);
    onClose();
  };

  return (
    <AlertDialog.Root open={target !== null} onOpenChange={(o) => !o && close()}>
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
          <AlertDialog.Title className={cn("m-0 font-serif font-normal", phone ? "text-[26px] leading-[1.15]" : "text-[30px]")}>
            Revoke &quot;{target?.name}&quot;?
          </AlertDialog.Title>
          <AlertDialog.Description asChild>
            <p className={cn("m-0", phone ? "text-[13px] leading-[1.6]" : "text-xs leading-[1.7]")}>
              {phone
                ? `Requests with ${target ? keyPrefix(target.id) : ""} are refused right away. Downloads already started keep running until their files expire.`
                : `Every request with ${target ? keyPrefix(target.id) : ""} will be refused right away with 401. Downloads it already started keep running; their files stay available until they expire.`}
            </p>
          </AlertDialog.Description>
          <p className={cn("m-0 text-muted", phone ? "text-xs" : "text-xs leading-[1.6]")}>
            {phone ? "This cannot be undone." : "This cannot be undone. Create a new key if the agent still needs access."}
          </p>
          {error && <ErrorLine>{error}</ErrorLine>}
          <div className={cn("mt-1.5", phone ? "mt-2 grid grid-cols-2 gap-2" : "flex flex-wrap justify-end gap-2.5")}>
            <AlertDialog.Cancel asChild>
              <Button type="button" variant="secondary" size="md" className={phone ? "h-12" : undefined}>
                Keep it
              </Button>
            </AlertDialog.Cancel>
            <Button type="button" variant="danger" size="md" className={phone ? "h-12" : undefined} disabled={busy} onClick={revoke}>
              {phone ? "Revoke" : "Revoke key"}
            </Button>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
