import { Menu } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { Layout } from "./breakpoint";
import type { Route } from "./route";
import { routePath } from "./route";

type NavProps = { layout: Layout; route: Route; email: string; go: (r: Route) => void; onSignOut: () => void };

const LINKS: { route: Route; label: string }[] = [
  { route: "keys", label: "API keys" },
  { route: "password", label: "Password" },
];

function Logo({ phone }: { phone: boolean }) {
  return (
    <div className={cn("flex items-baseline", phone ? "gap-2.5" : "gap-3.5")}>
      <b className={cn("font-serif font-normal tracking-[-0.02em]", phone ? "text-[26px]" : "text-[30px]")}>
        Re<em className="text-accent">Clip</em>
      </b>
      <span className={cn("tracking-[0.06em] text-muted uppercase", phone ? "text-[10px]" : "text-[11px]")}>Admin</span>
    </div>
  );
}

function PhoneBar({ route, email, go, onSignOut }: NavProps) {
  return (
    <header className="flex h-[60px] items-center justify-between border-b-[1.5px] border-line pr-2 pl-4">
      <Logo phone />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="icon" size="icon-touch" aria-label="Menu" className="text-fg">
            <Menu className="size-5" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuLabel>{email}</DropdownMenuLabel>
          {LINKS.map((l) => (
            <DropdownMenuItem key={l.route} className="min-h-11" onSelect={() => go(l.route)}>
              {l.label}
              {route === l.route && <span className="sr-only"> (current page)</span>}
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="danger" className="min-h-11" onSelect={onSignOut}>
            Sign out
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </header>
  );
}

function WideBar({ layout, route, email, go, onSignOut }: NavProps) {
  const tablet = layout === "tablet";
  return (
    <header
      className={cn(
        "flex flex-wrap items-center justify-between border-b-[1.5px] border-line",
        tablet ? "h-[72px] px-8" : "gap-x-[22px] gap-y-3 px-12 py-5",
      )}
    >
      <Logo phone={false} />
      <nav aria-label="Admin" className="flex flex-wrap items-center gap-x-[22px] gap-y-2.5 text-xs">
        {LINKS.map((l) => (
          <a
            key={l.route}
            href={routePath(l.route)}
            aria-current={route === l.route ? "page" : undefined}
            onClick={(e) => {
              e.preventDefault();
              go(l.route);
            }}
            className="no-underline aria-[current=page]:border-b-2 aria-[current=page]:border-accent aria-[current=page]:pb-0.5 aria-[current=page]:text-fg"
          >
            {l.label}
          </a>
        ))}
        {!tablet && <span className="text-[11px] text-muted">{email}</span>}
        <Button variant="ghost" onClick={onSignOut} className={tablet ? "h-9" : undefined}>
          Sign out
        </Button>
      </nav>
    </header>
  );
}

/** Signed-in frame: top bar (desktop/tablet nav or phone menu) around the page content. */
export function Shell({ children, ...nav }: NavProps & { children: ReactNode }) {
  const { layout } = nav;
  return (
    <div className="flex min-h-screen flex-col">
      {layout === "phone" ? <PhoneBar {...nav} /> : <WideBar {...nav} />}
      <main
        className={cn(
          "flex flex-col",
          layout === "desktop" && "gap-[18px] px-12 py-8",
          layout === "tablet" && "gap-5 p-8",
          layout === "phone" && "gap-4 px-4 pt-6 pb-10",
        )}
      >
        {children}
      </main>
    </div>
  );
}
