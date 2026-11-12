import { X } from "lucide-react";
import { createContext, useContext, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetTitle } from "@/components/ui/sheet";
import { cn } from "@/lib/utils";
import type { Layout } from "./breakpoint";

const SheetMode = createContext(false);

/** Dialog on desktop/tablet, bottom Sheet on phones. Children render inside; use ModalTitle for the heading. */
export function Modal({
  layout,
  open,
  onOpenChange,
  locked = false,
  className,
  children,
}: {
  layout: Layout;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Ignore Esc and outside clicks, so a one-time view cannot be dismissed by accident. */
  locked?: boolean;
  className?: string;
  children: ReactNode;
}) {
  const guard = locked
    ? { onEscapeKeyDown: (e: Event) => e.preventDefault(), onInteractOutside: (e: Event) => e.preventDefault() }
    : {};
  const phone = layout === "phone";
  return (
    <SheetMode.Provider value={phone}>
      {phone ? (
        <Sheet open={open} onOpenChange={onOpenChange}>
          <SheetContent {...guard} className={className}>
            {children}
          </SheetContent>
        </Sheet>
      ) : (
        <Dialog open={open} onOpenChange={onOpenChange}>
          <DialogContent {...guard} className={className}>
            {children}
          </DialogContent>
        </Dialog>
      )}
    </SheetMode.Provider>
  );
}

export function ModalTitle({ className, children, id }: { className?: string; children: ReactNode; id?: string }) {
  const sheet = useContext(SheetMode);
  const Title = sheet ? SheetTitle : DialogTitle;
  return (
    <Title id={id} className={className}>
      {children}
    </Title>
  );
}

export function ModalDescription({ className, children, id }: { className?: string; children: ReactNode; id?: string }) {
  const Desc = useContext(SheetMode) ? SheetDescription : DialogDescription;
  return (
    <Desc id={id} className={className}>
      {children}
    </Desc>
  );
}

/** Title row with a close button: plain icon on desktop, filled 40 px circle on phones. */
export function ModalHeader({ children }: { children: ReactNode }) {
  const sheet = useContext(SheetMode);
  const Close = sheet ? SheetClose : DialogClose;
  return (
    <div className="flex items-start justify-between gap-3">
      {children}
      <Close asChild>
        <Button
          type="button"
          variant="icon"
          size={sheet ? "icon-touch" : "icon"}
          aria-label="Close"
          className={cn(sheet && "bg-line-soft text-fg")}
        >
          <X className={sheet ? "size-4" : "size-[18px]"} />
        </Button>
      </Close>
    </div>
  );
}
