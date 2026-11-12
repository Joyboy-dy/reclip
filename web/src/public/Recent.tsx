import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { fileUrl, historyThumbUrl } from "../api/client";
import type { HistoryEntry } from "../api/types.gen";
import { historyNote, phoneHistoryNote } from "./describe";
import { fmtTtl } from "./format";
import { clean, downloadAgain, removeEntry } from "./history";
import { AgainIcon, ChevronIcon, CloseIcon, SaveIcon } from "./icons";
import { Thumb } from "./parts";
import { usePage } from "./store";

function CleanUpMenu({ phone }: { phone?: boolean }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className={cn(
            "inline-flex cursor-pointer items-center justify-between gap-2 rounded-lg border-[1.5px] border-line bg-card px-3 font-mono text-xs text-fg outline-none",
            "focus-visible:border-accent data-[state=open]:border-accent data-[state=open]:shadow-focus",
            phone ? "h-9" : "py-[7px]",
          )}
        >
          Clean up
          <ChevronIcon />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuItem onSelect={() => void clean({ mode: "expired" })}>Remove expired files from the list</DropdownMenuItem>
        <DropdownMenuItem onSelect={() => void clean({ mode: "older_than", days: 7 })}>Remove older than 7 days</DropdownMenuItem>
        <DropdownMenuItem onSelect={() => void clean({ mode: "older_than", days: 14 })}>Remove older than 14 days</DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem variant="danger" onSelect={() => void clean({ mode: "all" })}>
          Clear all history
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

const thumbOf = (e: HistoryEntry) => (e.has_thumbnail ? historyThumbUrl(e.id) : null);
const titleOf = (e: HistoryEntry) => e.title || "Untitled";

function Row({ e }: { e: HistoryEntry }) {
  return (
    <div data-history-row className="flex items-center gap-3 border-b border-line-soft px-3.5 py-2.5 last:border-b-0">
      <Thumb src={thumbOf(e)} className="h-10 w-16 rounded-md" />
      <div className="min-w-0 flex-1">
        <div className="truncate font-serif text-[15px]">{titleOf(e)}</div>
        <div className="text-[11px] text-muted">{historyNote(e)}</div>
      </div>
      {e.file_available ? (
        <Button asChild variant="done">
          <a href={fileUrl(e.id)} download="">
            Save
          </a>
        </Button>
      ) : (
        <Button type="button" variant="ghost" onClick={() => downloadAgain(e)}>
          Download again
        </Button>
      )}
      <Button type="button" variant="icon" size="icon" aria-label="Remove from history" onClick={() => void removeEntry(e.id)}>
        <CloseIcon />
      </Button>
    </div>
  );
}

function PhoneRow({ e }: { e: HistoryEntry }) {
  const icon = "size-10 rounded-lg";
  return (
    <div data-history-row className="flex min-h-16 items-center gap-3 border-b border-line-soft py-2.5 pr-2 pl-3 last:border-b-0">
      <Thumb src={thumbOf(e)} className="h-9 w-14 rounded-md" />
      <div className="min-w-0 flex-1">
        <div className="truncate font-serif text-[15px]">{titleOf(e)}</div>
        <div className="mt-0.5 truncate text-[11px] text-muted">{phoneHistoryNote(e)}</div>
      </div>
      {e.file_available ? (
        <Button asChild variant="icon" size="icon-touch" className={cn(icon, "text-success hover:text-success")}>
          <a href={fileUrl(e.id)} download="" aria-label="Save">
            <SaveIcon />
          </a>
        </Button>
      ) : (
        <Button type="button" variant="icon" size="icon-touch" className={icon} aria-label="Download again" onClick={() => downloadAgain(e)}>
          <AgainIcon />
        </Button>
      )}
      <Button type="button" variant="icon" size="icon-touch" className={icon} aria-label="Remove from history" onClick={() => void removeEntry(e.id)}>
        <CloseIcon />
      </Button>
    </div>
  );
}

/** "Recent downloads": the visitor's finished downloads, kept as titles only. */
export function History({ phone }: { phone?: boolean }) {
  const history = usePage((s) => s.history);
  const config = usePage((s) => s.config);
  const days = config?.history_days ?? 30;
  const ttl = fmtTtl(config?.file_ttl_secs ?? 21600);
  const entries = history.status === "ready" ? history.entries : [];
  const error = history.status === "loading" ? undefined : history.error;
  const empty =
    history.status === "loading" ? "Loading..." : history.status === "error" ? undefined : entries.length === 0 ? "No downloads yet." : undefined;
  const Line = phone ? PhoneRow : Row;

  const list = (
    <div className="overflow-hidden rounded-card border-[1.5px] border-line bg-card">
      {entries.map((e) => (
        <Line key={e.id} e={e} />
      ))}
      {empty && <p className="m-0 px-3.5 py-3 text-[11px] text-muted">{empty}</p>}
      {error && (
        <p role="alert" className="m-0 px-3.5 py-3 text-[11px] text-error">
          {history.status === "error" ? `Could not load the history: ${error}` : error}
        </p>
      )}
    </div>
  );

  if (phone) {
    return (
      <section aria-labelledby="hist-title" className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <h2 id="hist-title" className="m-0 text-[11px] font-medium tracking-[0.06em] text-muted uppercase">
            Recent downloads
          </h2>
          <CleanUpMenu phone />
        </div>
        {list}
        <p className="m-0 text-[11px] leading-[1.5] text-muted">
          Files stay downloadable for {ttl}. The list keeps titles for {days} days.
        </p>
      </section>
    );
  }
  return (
    <section aria-labelledby="hist-title" className="mt-10">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
        <h2 id="hist-title" className="m-0 text-xs font-medium tracking-[0.06em] uppercase">
          Recent downloads
        </h2>
        <CleanUpMenu />
      </div>
      <p className="m-0 mb-2.5 text-[11px] leading-[1.5] text-muted">
        Last {days} days. Files stay downloadable for {ttl}; the list keeps titles only.
      </p>
      {list}
    </section>
  );
}
