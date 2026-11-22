import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import type { ServerStats } from "@/api/types.gen";
import { cn } from "@/lib/utils";
import { ProgressBar } from "../public/parts";
import { bytes, duration, share } from "./activity-lib";
import { adminErrorText, getServer, isUnauthorized } from "./api";
import type { Layout } from "./breakpoint";
import { ErrorLine, PageTitle } from "./parts";
import { Pair } from "./users-page";

/** Refresh period while the tab is visible; nothing is asked while it is hidden. */
const EVERY_MS = 5000;
const H2 = "m-0 mt-2 font-serif text-[24px] font-normal tracking-[-0.01em]";

function Metric({ label, value, sub, bar }: { label: string; value: ReactNode; sub?: string; bar?: number | null }) {
  return (
    <div className="flex flex-col gap-1.5 rounded-[14px] border-[1.5px] border-line bg-card p-4">
      <span className="text-[11px] tracking-[0.04em] text-muted uppercase">{label}</span>
      <span className="font-serif text-[30px] leading-none tracking-[-0.01em]">{value}</span>
      {bar !== undefined && bar !== null && (
        <div className="pt-1">
          <ProgressBar value={bar} />
        </div>
      )}
      {/* At the bottom: the lines of a row stay aligned, bar or not. */}
      {sub && <span className="mt-auto pt-0.5 text-[11px] leading-[1.5] text-muted">{sub}</span>}
    </div>
  );
}

const NONE = "n/a";

export function ServerPage({ layout, onUnauthorized }: { layout: Layout; onUnauthorized: () => void }) {
  const [stats, setStats] = useState<ServerStats | null>(null);
  // Rates need two samples: until then a missing one is still being measured.
  const [samples, setSamples] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(async () => {
    try {
      setStats(await getServer());
      setSamples((n) => n + 1);
      setError(null);
    } catch (err) {
      if (isUnauthorized(err)) return onUnauthorized();
      setError(adminErrorText(err));
    }
  }, [onUnauthorized]);

  useEffect(() => {
    let stopped = false;
    const tick = async () => {
      if (stopped) return;
      if (document.visibilityState === "visible") await load();
      if (!stopped) timer.current = setTimeout(() => void tick(), EVERY_MS);
    };
    const onShow = () => {
      if (document.visibilityState !== "visible") return;
      if (timer.current) clearTimeout(timer.current);
      void tick();
    };
    void tick();
    document.addEventListener("visibilitychange", onShow);
    return () => {
      stopped = true;
      if (timer.current) clearTimeout(timer.current);
      document.removeEventListener("visibilitychange", onShow);
    };
  }, [load]);

  const measuring = samples < 2;
  const live = (n: number | null, show: (n: number) => string) => (n !== null ? show(n) : measuring ? "…" : NONE);
  const rate = (n: number | null) => live(n, (v) => `${bytes(v)}/s`);
  const cols = layout === "desktop" ? "grid-cols-3" : layout === "tablet" ? "grid-cols-2" : "grid-cols-1";
  const s = stats;

  return (
    <>
      <PageTitle title="Server">
        {layout === "tablet" ? "Load, files and jobs, every 5 s." : "Load of the container, files waiting to be saved, and jobs. Refreshed every 5 s while this tab is open."}
      </PageTitle>
      {error && <ErrorLine>{error}</ErrorLine>}
      {!s && !error && <p className="m-0 text-xs text-muted">Loading…</p>}
      {s && (
        <>
          <div className={cn("grid gap-3", cols)}>
            <Metric
              label="CPU"
              value={live(s.cpu_percent, (v) => `${Math.round(v)}%`)}
              sub={`${s.cpu_cores} ${s.cpu_cores === 1 ? "core" : "cores"}${s.cpu_percent === null && measuring ? " · measuring" : ""}`}
              bar={s.cpu_percent}
            />
            <Metric
              label="Memory"
              value={s.memory_bytes === null ? NONE : bytes(s.memory_bytes)}
              sub={s.memory_limit ? `of ${bytes(s.memory_limit)}` : "No container limit"}
              bar={share(s.memory_bytes, s.memory_limit)}
            />
            <Metric
              label="Files on disk"
              value={bytes(s.files_bytes)}
              sub={`of ${bytes(s.files_limit)} budget`}
              bar={share(s.files_bytes, s.files_limit)}
            />
            <Metric label="Jobs" value={<Pair a={s.jobs_running} b={s.jobs_queued} />} sub="Running / waiting" />
            <Metric
              label="Network"
              value={rate(s.net_rx_rate)}
              sub={`In / out ${rate(s.net_tx_rate)}`}
            />
            <Metric label="Uptime" value={duration(s.uptime_secs)} sub="Since the last start" />
          </div>

          <h2 className={H2}>Average download</h2>
          <div className={cn("grid gap-3", cols)}>
            <Metric
              label="Time"
              value={s.avg_duration_secs === null ? NONE : duration(s.avg_duration_secs)}
              sub="From start to file ready"
            />
            <Metric
              label="CPU time"
              value={s.avg_cpu_secs === null ? NONE : `${s.avg_cpu_secs.toFixed(1)} s`}
              sub="ReClip, yt-dlp and ffmpeg, since the last start"
            />
            <Metric
              label="Size"
              value={s.avg_size_bytes === null ? NONE : bytes(s.avg_size_bytes)}
              sub={`${s.downloads_done} ${s.downloads_done === 1 ? "download" : "downloads"} finished`}
            />
          </div>
          <p className="m-0 text-[11px] leading-normal text-muted">
            CPU, memory and network are the container&apos;s (Linux). Averages cover the finished downloads still in the history.
          </p>
        </>
      )}
    </>
  );
}
