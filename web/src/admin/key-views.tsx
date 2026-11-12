import { CalendarDays } from "lucide-react";
import type { ApiKeyInfo } from "@/api/types.gen";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { Layout } from "./breakpoint";
import { capitalize, expiryInfo, keyPrefix, keyStatus, limitsLabel, relativeTime, rightsLabel, type KeyStatus } from "./lib";

type Props = { keys: ApiKeyInfo[]; now: Date; onRevoke: (key: ApiKeyInfo) => void };

const BADGE: Record<KeyStatus, { label: string; variant: "success" | "off" | "accent" }> = {
  active: { label: "Active", variant: "success" },
  expired: { label: "Expired", variant: "accent" },
  revoked: { label: "Revoked", variant: "off" },
};

function StatusBadge({ status }: { status: KeyStatus }) {
  return <Badge variant={BADGE[status].variant}>{BADGE[status].label}</Badge>;
}

/** Calendar icon with a tooltip (date + days left). Orange within 7 days, red once expired. */
function ExpiryIcon({ expiresAt, now }: { expiresAt: string | null; now: Date }) {
  const info = expiryInfo(expiresAt, now);
  if (!info) return null;
  const when = info.status === "expired" ? `Expired ${info.date}` : `Expires ${info.date}`;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="icon"
          aria-label={info.status === "expired" ? when : `${when}, ${info.text}`}
          className={cn("size-7", info.status === "soon" && "text-accent-ink", info.status === "expired" && "text-error")}
        >
          <CalendarDays className="size-4" aria-hidden />
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        {when}
        {info.status !== "expired" && (
          <>
            <br />
            {info.text}
          </>
        )}
      </TooltipContent>
    </Tooltip>
  );
}

const TH = "border-b border-line-soft px-4 py-3 text-left text-[11px] font-medium tracking-[0.04em] text-muted uppercase";

export function KeysTable({ keys, now, onRevoke }: Props) {
  return (
    <div className="overflow-x-auto rounded-[14px] border-[1.5px] border-line bg-card">
      <table className="w-full border-collapse text-xs">
        <thead>
          <tr>
            {["Name", "Key", "Rights", "Limits", "Expires", "Last used", "Uses", "Status"].map((h) => (
              <th key={h} scope="col" className={TH}>
                {h}
              </th>
            ))}
            <th scope="col" className={TH}>
              <span className="sr-only">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody className="[&>tr:last-child>td]:border-b-0">
          {keys.map((k) => {
            const status = keyStatus(k, now);
            const off = status !== "active";
            return (
              <tr key={k.id} className={cn(off && "text-muted")}>
                <td className="border-b border-line-soft px-4 py-3.5 font-serif text-base">{k.name}</td>
                <td className={cn("border-b border-line-soft px-4 py-3.5", !off && "text-muted")}>{keyPrefix(k.id)}</td>
                <td className="border-b border-line-soft px-4 py-3.5">{rightsLabel(k.scopes)}</td>
                <td className="border-b border-line-soft px-4 py-3.5">{limitsLabel(k)}</td>
                <td className="border-b border-line-soft px-4 py-3.5">
                  <ExpiryIcon expiresAt={k.expires_at} now={now} />
                </td>
                <td className={cn("border-b border-line-soft px-4 py-3.5", !off && "text-muted")}>{relativeTime(k.last_used_at, now)}</td>
                <td className="border-b border-line-soft px-4 py-3.5">{k.request_count.toLocaleString("en-US")}</td>
                <td className="border-b border-line-soft px-4 py-3.5">
                  <StatusBadge status={status} />
                </td>
                <td className="border-b border-line-soft px-4 py-3.5 text-right">
                  {status === "active" && (
                    <Button
                      type="button"
                      variant="icon"
                      onClick={() => onRevoke(k)}
                      aria-label={`Revoke ${k.name}`}
                      className="h-7 w-auto px-2 text-[11px] tracking-[0.03em] text-error uppercase hover:text-error"
                    >
                      Revoke
                    </Button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function KeyCard({ k, now, tablet, onRevoke }: { k: ApiKeyInfo; now: Date; tablet: boolean; onRevoke: (key: ApiKeyInfo) => void }) {
  const status = keyStatus(k, now);
  const info = expiryInfo(k.expires_at, now);
  const revoked = status === "revoked";
  const facts: [string, string][] = revoked
    ? [
        ["Rights", rightsLabel(k.scopes, true)],
        ["Last used", capitalize(relativeTime(k.last_used_at, now))],
      ]
    : [
        ["Rights", rightsLabel(k.scopes, true)],
        ["Limits", limitsLabel(k, true)],
        ["Last used", capitalize(relativeTime(k.last_used_at, now))],
        ["Uses", k.request_count.toLocaleString("en-US")],
      ];
  const expiryText = info
    ? info.status === "expired"
      ? `Expired ${info.date}`
      : `${info.date} · ${tablet ? info.text.replace(" left", "") : info.text}`
    : "No expiry";

  return (
    <article
      className={cn(
        "flex flex-col gap-3.5 rounded-[14px] border-[1.5px] border-line p-4",
        revoked ? "bg-transparent" : "bg-card",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className={cn("font-serif text-lg leading-[1.2]", revoked && "text-muted")}>{k.name}</div>
          <div className="mt-1 text-[11px] text-muted">{keyPrefix(k.id)}</div>
        </div>
        <StatusBadge status={status} />
      </div>
      <dl className={cn("m-0 grid grid-cols-2 gap-x-4 gap-y-3", revoked && "text-muted")}>
        {facts.map(([term, value]) => (
          <div key={term} className="min-w-0">
            <dt className="text-[10px] tracking-[0.05em] text-muted uppercase">{term}</dt>
            <dd className="mt-[3px] ml-0 text-xs">{value}</dd>
          </div>
        ))}
      </dl>
      {!revoked && (
        <div className="flex min-h-9 items-center justify-between gap-3 border-t border-line-soft pt-3">
          <span
            className={cn(
              "inline-flex items-center gap-1.5 text-[11px] text-muted",
              info?.status === "soon" && "text-accent-ink",
              info?.status === "expired" && "text-error",
            )}
          >
            {info && <CalendarDays className="size-3.5 shrink-0" aria-hidden />}
            {expiryText}
          </span>
          <Button
            type="button"
            variant="icon"
            onClick={() => onRevoke(k)}
            aria-label={`Revoke ${k.name}`}
            className="h-11 w-auto px-2 text-xs tracking-[0.04em] text-error uppercase hover:text-error"
          >
            Revoke
          </Button>
        </div>
      )}
    </article>
  );
}


/** Tablet: two-column grid. Phone: single column. */
export function KeyCards({ keys, now, onRevoke, layout }: Props & { layout: Layout }) {
  return (
    <div className={cn("grid gap-4", layout === "tablet" ? "grid-cols-2" : "grid-cols-1")}>
      {keys.map((k) => (
        <KeyCard key={k.id} k={k} now={now} tablet={layout === "tablet"} onRevoke={onRevoke} />
      ))}
    </div>
  );
}
