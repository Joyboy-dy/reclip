import { createRoot } from "react-dom/client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { initNonce } from "@/lib/nonce";

// Must run before the first render so Radix style tags carry the CSP nonce.
initNonce();

// Minimal shell proving the React + Tailwind + shadcn chain; the real admin app lands in a later task.
function AdminShell() {
  return (
    <TooltipProvider>
      <header className="flex flex-wrap items-center justify-between gap-x-[22px] gap-y-3 border-b-[1.5px] border-line px-12 py-5 max-[560px]:h-[60px] max-[560px]:px-4 max-[560px]:py-0">
        <div className="flex items-baseline gap-3.5 max-[560px]:gap-2.5">
          <b className="font-serif text-[30px] font-normal tracking-[-0.02em] max-[560px]:text-[26px]">
            Re<em className="text-accent">Clip</em>
          </b>
          <span className="text-[11px] tracking-[0.06em] text-muted uppercase">Admin</span>
        </div>
      </header>
    </TooltipProvider>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(<AdminShell />);
