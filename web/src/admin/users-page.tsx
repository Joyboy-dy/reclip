import { Search } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import type { ActivityReport, BlockInfo, ClientInfo, SharedIp } from "@/api/types.gen";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { FILTERS, FLAG_HINTS, FLAG_LABELS, type Filter, blockTarget, bytes, clientName, clientSub, filterClients } from "./activity-lib";
import { adminErrorText, getActivity, isUnauthorized, listBlocks, removeBlock } from "./api";
import { type BlockAsk, BlockDialog } from "./block-dialog";
import type { Layout } from "./breakpoint";
import { capitalize, formatDate, relativeTime } from "./lib";
import { ErrorLine, PageTitle, TextInput } from "./parts";

type Props = { layout: Layout; onUnauthorized: () => void };

const TH = "border-b border-line-soft px-4 py-3 text-left text-[11px] font-medium tracking-[0.04em] whitespace-nowrap text-muted uppercase";
const TD = "border-b border-line-soft px-4 py-3.5 align-middle";
const NUM = "text-right tabular-nums";
const H2 = "m-0 mt-2 font-serif text-[24px] font-normal tracking-[-0.01em]";

/** One figure of the signals row; `alert` tints it when it needs a look. */
function Tile({ label, value, sub, alert }: { label: string; value: ReactNode; sub: string; alert?: boolean }) {
  return (
    <div className={cn("flex flex-col gap-1 rounded-[14px] border-[1.5px] p-4", alert ? "border-accent/40 bg-accent-soft" : "border-line bg-card")}>
      <span className={cn("text-[11px] tracking-[0.04em] uppercase", alert ? "text-accent-ink" : "text-muted")}>{label}</span>
      <span className="font-serif text-[30px] leading-none tracking-[-0.01em]">{value}</span>
      <span className={cn("mt-auto text-[11px] leading-[1.5]", alert ? "text-accent-ink" : "text-muted")}>{sub}</span>
    </div>
  );
}

/** Two figures of one tile, "2 / 14". */
export function Pair({ a, b }: { a: number; b: number }) {
  return (
    <>
      {a}
      <span aria-hidden className="mx-1.5 align-[0.12em] font-mono text-[18px] text-muted">
        /
      </span>
      <span className="sr-only">and</span>
      {b}
    </>
  );
}

/** A badge with its explanation on hover or focus. */
function HintBadge({ variant, label, hint }: { variant: "off" | "accent"; label: string; hint: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" className="cursor-help rounded-md outline-none focus-visible:shadow-[0_0_0_2px_var(--color-accent)]">
          <Badge variant={variant}>{label}</Badge>
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-[260px]">{hint}</TooltipContent>
    </Tooltip>
  );
}

function blockHint(b: BlockInfo, now: Date): string {
  const by = b.created_by ? ` by ${b.created_by}` : "";
  const why = b.reason ? `: ${b.reason}` : "";
  return `Blocked ${relativeTime(b.created_at, now)}${by}${why}`;
}

function Status({ c, now }: { c: ClientInfo; now: Date }) {
  if (!c.block && c.flags.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {c.block && <HintBadge variant="off" label="Blocked" hint={blockHint(c.block, now)} />}
      {c.flags.map((f) => (
        <HintBadge key={f} variant="accent" label={FLAG_LABELS[f]} hint={FLAG_HINTS[f]} />
      ))}
    </div>
  );
}

/** The last address, and the others in a tooltip. */
function Addresses({ c, now }: { c: ClientInfo; now: Date }) {
  if (!c.last_ip) return <span className="text-muted">{c.kind === "telegram" ? "through the bot" : "none"}</span>;
  const others = c.ips_total - 1;
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className="truncate">{c.last_ip}</span>
      {others > 0 && (
        <Tooltip>
          <TooltipTrigger asChild>
            <button type="button" className="cursor-help rounded-md bg-line-soft px-1.5 py-px text-[11px] text-muted outline-none focus-visible:shadow-[0_0_0_2px_var(--color-accent)]">
              +{others}
            </button>
          </TooltipTrigger>
          <TooltipContent className="max-w-[320px]">
            {c.ips.map((ip) => (
              <div key={ip.ip} className="flex justify-between gap-4">
                <span>{ip.ip}</span>
                <span className="opacity-70">{relativeTime(ip.last_seen, now)}</span>
              </div>
            ))}
            {c.ips_total > c.ips.length && <div className="opacity-70">and {c.ips_total - c.ips.length} more</div>}
          </TooltipContent>
        </Tooltip>
      )}
    </span>
  );
}

type Act = { block: (a: BlockAsk) => void; unblock: (b: BlockInfo) => void };

/** Block / Unblock as a quiet text button (`touch`: 44 px tall). */
function Action({ c, act, touch }: { c: ClientInfo; act: Act; touch?: boolean }) {
  const own = c.block?.kind === "owner" ? c.block : null;
  const cls = cn("w-auto px-2 tracking-[0.04em] uppercase", touch ? "h-11 text-xs" : "h-7 text-[11px]");
  if (own) {
    return (
      <Button type="button" variant="icon" className={cn(cls, "text-fg")} aria-label={`Unblock ${clientName(c)}`} onClick={() => act.unblock(own)}>
        Unblock
      </Button>
    );
  }
  return (
    <Button
      type="button"
      variant="icon"
      className={cn(cls, "text-error hover:text-error")}
      aria-label={`Block ${clientName(c)}`}
      onClick={() => act.block({ kind: "owner", target: c.owner, name: clientName(c) })}
    >
      Block
    </Button>
  );
}

function ClientsTable({ clients, now, act }: { clients: ClientInfo[]; now: Date; act: Act }) {
  return (
    <div className="overflow-x-auto rounded-[14px] border-[1.5px] border-line bg-card">
      <table className="w-full border-collapse text-xs">
        <thead>
          <tr>
            <th scope="col" className={TH}>User</th>
            <th scope="col" className={TH}>Address</th>
            <th scope="col" className={cn(TH, "text-right")} title="Downloads in the last 24 hours">24 h</th>
            <th scope="col" className={cn(TH, "text-right")} title="Downloads in the last 7 days">7 d</th>
            <th scope="col" className={cn(TH, "text-right")} title="Downloads in the retention window">All</th>
            <th scope="col" className={cn(TH, "text-right")}>Volume</th>
            <th scope="col" className={TH}>Last seen</th>
            <th scope="col" className={TH}>Status</th>
            <th scope="col" className={TH}>
              <span className="sr-only">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody className="[&>tr:last-child>td]:border-b-0">
          {clients.map((c) => (
            <tr key={c.owner} className={cn(c.block && "text-muted")}>
              <td className={cn(TD, "max-w-[280px]")}>
                <div className={cn("truncate font-serif text-[15px] leading-[1.3]", !c.block && "text-fg")}>{clientName(c)}</div>
                <div className="mt-0.5 truncate text-[11px] text-muted">{clientSub(c)}</div>
              </td>
              <td className={cn(TD, "max-w-[220px]")}>
                <Addresses c={c} now={now} />
              </td>
              <td className={cn(TD, NUM)}>{c.downloads_day}</td>
              <td className={cn(TD, NUM)}>{c.downloads_week}</td>
              <td className={cn(TD, NUM)}>{c.downloads_total}</td>
              <td className={cn(TD, NUM, "whitespace-nowrap")}>{bytes(c.bytes_total)}</td>
              <td className={cn(TD, "whitespace-nowrap text-muted")}>{relativeTime(c.last_seen, now)}</td>
              <td className={TD}>
                <Status c={c} now={now} />
              </td>
              <td className={cn(TD, "text-right")}>
                <Action c={c} act={act} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Fact({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-[10px] tracking-[0.05em] text-muted uppercase">{term}</dt>
      <dd className="mt-[3px] ml-0 truncate text-xs">{children}</dd>
    </div>
  );
}

function ClientCard({ c, now, act }: { c: ClientInfo; now: Date; act: Act }) {
  return (
    <article className={cn("flex flex-col gap-3.5 rounded-[14px] border-[1.5px] border-line p-4", c.block ? "bg-transparent" : "bg-card")}>
      <div className="min-w-0">
        <div className={cn("truncate font-serif text-lg leading-[1.2]", c.block && "text-muted")}>{clientName(c)}</div>
        <div className="mt-1 truncate text-[11px] text-muted">{clientSub(c)}</div>
      </div>
      <dl className="m-0 grid grid-cols-2 gap-x-4 gap-y-3">
        <Fact term="Address">
          <Addresses c={c} now={now} />
        </Fact>
        <Fact term="Last seen">{capitalize(relativeTime(c.last_seen, now))}</Fact>
        <Fact term="Downloads">
          {c.downloads_day} · {c.downloads_week} · {c.downloads_total}
        </Fact>
        <Fact term="Volume">{bytes(c.bytes_total)}</Fact>
      </dl>
      <div className="flex min-h-11 items-center justify-between gap-3 border-t border-line-soft pt-3">
        <Status c={c} now={now} />
        <span className="ml-auto">
          <Action c={c} act={act} touch />
        </span>
      </div>
    </article>
  );
}

function SharedIps({ list, now, act, layout }: { list: SharedIp[]; now: Date; act: Act; layout: Layout }) {
  const button = (s: SharedIp, touch: boolean) => {
    const cls = cn("w-auto px-2 tracking-[0.04em] uppercase", touch ? "h-11 text-xs" : "h-7 text-[11px]");
    return s.block ? (
      <Button type="button" variant="icon" className={cn(cls, "text-fg")} onClick={() => s.block && act.unblock(s.block)}>
        Unblock
      </Button>
    ) : (
      <Button
        type="button"
        variant="icon"
        className={cn(cls, "text-error hover:text-error")}
        onClick={() => act.block({ kind: "ip", target: s.ip, name: s.ip })}
      >
        Block
      </Button>
    );
  };
  if (layout !== "desktop") {
    return (
      <div className={cn("grid gap-3", layout === "tablet" ? "grid-cols-2" : "grid-cols-1")}>
        {list.map((s) => (
          <div key={s.ip} className="flex items-center justify-between gap-3 rounded-[14px] border-[1.5px] border-line bg-card py-2 pr-2 pl-4">
            <div className="min-w-0">
              <div className="truncate text-xs">{s.ip}</div>
              <div className="mt-0.5 text-[11px] text-muted">
                {s.devices} browsers · {relativeTime(s.last_seen, now)}
              </div>
            </div>
            {button(s, true)}
          </div>
        ))}
      </div>
    );
  }
  return (
    <div className="overflow-x-auto rounded-[14px] border-[1.5px] border-line bg-card">
      <table className="w-full border-collapse text-xs">
        <thead>
          <tr>
            <th scope="col" className={TH}>Address</th>
            <th scope="col" className={cn(TH, "text-right")}>Browsers (7 d)</th>
            <th scope="col" className={TH}>Last seen</th>
            <th scope="col" className={TH}>Status</th>
            <th scope="col" className={TH}>
              <span className="sr-only">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody className="[&>tr:last-child>td]:border-b-0">
          {list.map((s) => (
            <tr key={s.ip}>
              <td className={TD}>{s.ip}</td>
              <td className={cn(TD, NUM)}>{s.devices}</td>
              <td className={cn(TD, "text-muted")}>{relativeTime(s.last_seen, now)}</td>
              <td className={TD}>{s.block && <HintBadge variant="off" label="Blocked" hint={blockHint(s.block, now)} />}</td>
              <td className={cn(TD, "text-right")}>{button(s, false)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Blocks({ blocks, now, act, layout }: { blocks: BlockInfo[]; now: Date; act: Act; layout: Layout }) {
  return (
    <ul className={cn("m-0 grid list-none gap-3 p-0", layout === "desktop" ? "grid-cols-3" : layout === "tablet" ? "grid-cols-2" : "grid-cols-1")}>
      {blocks.map((b) => (
        <li key={b.id} className="flex items-center justify-between gap-3 rounded-[14px] border-[1.5px] border-line bg-card py-2 pr-2 pl-4">
          <div className="min-w-0 py-1">
            <div className="truncate text-xs">{blockTarget(b)}</div>
            <div className="mt-0.5 truncate text-[11px] text-muted">
              {b.reason || "No reason given"} · {formatDate(b.created_at)}
            </div>
          </div>
          <Button
            type="button"
            variant="icon"
            className={cn("w-auto shrink-0 px-2 tracking-[0.04em] text-fg uppercase", layout === "phone" ? "h-11 text-xs" : "h-7 text-[11px]")}
            aria-label={`Unblock ${blockTarget(b)}`}
            onClick={() => act.unblock(b)}
          >
            Unblock
          </Button>
        </li>
      ))}
    </ul>
  );
}

export function UsersPage({ layout, onUnauthorized }: Props) {
  const [report, setReport] = useState<ActivityReport | null>(null);
  const [blocks, setBlocks] = useState<BlockInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [ask, setAsk] = useState<BlockAsk | null>(null);

  const load = useCallback(async () => {
    try {
      const [r, b] = await Promise.all([getActivity(), listBlocks()]);
      setReport(r);
      setBlocks(b);
      setError(null);
    } catch (err) {
      if (isUnauthorized(err)) return onUnauthorized();
      setError(adminErrorText(err));
    }
  }, [onUnauthorized]);

  useEffect(() => {
    void load();
  }, [load]);

  const act: Act = {
    block: setAsk,
    unblock: async (b) => {
      try {
        await removeBlock(b.id);
        await load();
      } catch (err) {
        if (isUnauthorized(err)) return onUnauthorized();
        setError(adminErrorText(err));
      }
    },
  };

  const clients = useMemo(() => (report ? filterClients(report.clients, filter, query) : []), [report, filter, query]);
  const now = new Date();
  const phone = layout === "phone";
  const flagged = report?.clients.filter((c) => c.flags.length > 0).length ?? 0;

  return (
    <>
      <div className={phone ? "flex flex-col gap-4" : "flex items-end justify-between gap-6"}>
        <PageTitle title="Users">
          {layout === "tablet"
            ? "Who uses ReClip, from where, and how much."
            : "Who uses ReClip, from where, and how much. Block a browser, a Telegram account, a key or an address."}
        </PageTitle>
        <Button type="button" variant="secondary" size={phone ? "touch" : "md"} className={cn("shrink-0", phone && "w-full")} onClick={() => setAsk({ kind: "ip" })}>
          Block an address
        </Button>
      </div>

      {error && <ErrorLine>{error}</ErrorLine>}
      {!report && !error && <p className="m-0 text-xs text-muted">Loading activity…</p>}

      {report && (
        <>
          <div className={cn("grid gap-3", phone || layout === "tablet" ? "grid-cols-2" : "grid-cols-4")}>
            <Tile
              label="New browsers"
              value={<Pair a={report.new_devices_hour} b={report.new_devices_day} />}
              sub={report.burst ? "Unusual this hour" : "Hour / 24 h"}
              alert={report.burst}
            />
            <Tile label="Shared addresses" value={String(report.shared_ips.length)} sub="4+ browsers, 7 days" alert={report.shared_ips.length > 0} />
            <Tile label="Flagged" value={String(flagged)} sub="May share the link" alert={flagged > 0} />
            <Tile label="Blocked" value={String(blocks.length)} sub="Users and addresses" />
          </div>

          <div className={cn("flex gap-3", phone ? "flex-col" : "items-center justify-between")}>
            <div className={cn(phone && "-mx-4 overflow-x-auto px-4 [scrollbar-width:none]")}>
              <ToggleGroup
                type="single"
                aria-label="Show"
                tone="soft"
                value={filter}
                onValueChange={(v) => v && setFilter(v as Filter)}
                className="w-max"
              >
                {FILTERS.map((f) => (
                  <ToggleGroupItem key={f.value} value={f.value} className={phone ? "h-11 px-3.5" : "px-3.5"}>
                    {f.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>
            <div className={cn("relative", phone ? "w-full" : "w-[280px]")}>
              <Search aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted" />
              <label htmlFor="users-search" className="sr-only">
                Search users
              </label>
              <TextInput
                id="users-search"
                type="search"
                value={query}
                placeholder="Name, device or address"
                onChange={(e) => setQuery(e.target.value)}
                className="pl-8"
              />
            </div>
          </div>

          {clients.length === 0 ? (
            <p className="m-0 text-xs text-muted">
              {report.clients.length === 0 ? "Nobody used ReClip in this period yet." : "No user matches this filter."}
            </p>
          ) : layout === "desktop" ? (
            <ClientsTable clients={clients} now={now} act={act} />
          ) : (
            <div className={cn("grid gap-4", layout === "tablet" ? "grid-cols-2" : "grid-cols-1")}>
              {clients.map((c) => (
                <ClientCard key={c.owner} c={c} now={now} act={act} />
              ))}
            </div>
          )}

          {report.shared_ips.length > 0 && (
            <>
              <h2 className={H2}>Shared addresses</h2>
              <p className="m-0 -mt-2 text-xs leading-[1.6] text-muted">
                Addresses 4 or more browsers came from this week: a household or an office, or a link passed around.
              </p>
              <SharedIps list={report.shared_ips} now={now} act={act} layout={layout} />
            </>
          )}

          {blocks.length > 0 && (
            <>
              <h2 className={H2}>Blocked</h2>
              <Blocks blocks={blocks} now={now} act={act} layout={layout} />
            </>
          )}

          <p className="m-0 text-[11px] leading-normal text-muted">
            Activity and addresses are kept {report.retention_days} days, then deleted. On the web, a user is a browser: clearing its
            cookies makes it a new one. Downloads: last 24 h, 7 days and the whole period.
          </p>
        </>
      )}

      <BlockDialog
        layout={layout}
        ask={ask}
        onUnauthorized={onUnauthorized}
        onClose={() => setAsk(null)}
        onBlocked={() => {
          setAsk(null);
          void load();
        }}
      />
    </>
  );
}
