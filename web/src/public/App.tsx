import { Button } from "@/components/ui/button";
import { CardView } from "./Card";
import { downloadAll } from "./downloads";
import { ClipOptions, FetchButton, LinkBox, type Links, MediaControls, useLinks } from "./Form";
import { History } from "./Recent";
import { TelegramIcon } from "./icons";
import { usePhone } from "./parts";
import { PhoneCard } from "./PhoneCard";
import { usePage } from "./store";

function Telegram({ phone }: { phone?: boolean }) {
  const bot = usePage((s) => s.config?.telegram_bot_username);
  if (!bot) return null;
  return (
    <div className={phone ? "mt-3" : undefined}>
      <a
        href={`https://t.me/${encodeURIComponent(bot)}`}
        target="_blank"
        rel="noopener"
        className={phone ? "inline-flex min-h-11 items-center gap-1.5 no-underline" : "mt-2.5 inline-flex items-center gap-1.5 no-underline"}
      >
        <TelegramIcon />
        Use on Telegram
      </a>
    </div>
  );
}

/** "Download all" once several fetched links are ready. */
function DownloadAll({ phone }: { phone?: boolean }) {
  const ready = usePage((s) => s.cards.filter((c) => c.status === "ready").length);
  if (ready < 2) return null;
  return (
    <Button
      type="button"
      variant="secondary"
      size={phone ? "touch" : "md"}
      className={phone ? "h-10 w-full rounded-lg" : "w-full"}
      onClick={() => void downloadAll()}
    >
      Download all ({ready})
    </Button>
  );
}

function Desktop({ links }: { links: Links }) {
  const cards = usePage((s) => s.cards);
  return (
    <main className="mx-auto max-w-[620px] px-6 pt-[60px] pb-20">
      <div className="mb-10">
        <h1 className="m-0 font-serif text-[67px] leading-none font-normal tracking-[-0.03em]">
          Re<em className="text-accent italic">Clip</em>
        </h1>
        <p className="m-0 mt-2.5 text-xs tracking-[0.04em] text-muted uppercase">Free media downloader</p>
      </div>
      <LinkBox links={links} />
      <p className="m-0 mb-[18px] text-[11px] leading-[1.5] text-muted">Multiple links? Separate with spaces, commas, or newlines.</p>
      <div className="mb-3 flex flex-wrap items-center gap-2.5">
        <MediaControls />
        <FetchButton links={links} />
      </div>
      <ClipOptions />
      <div className="flex flex-col gap-2.5">
        <DownloadAll />
        {cards.map((c) => (
          <CardView key={c.key} c={c} />
        ))}
      </div>
      <History />
      <footer className="mt-10 text-center text-[11px] leading-[1.8] tracking-[0.05em] text-muted uppercase">
        YouTube · TikTok · Instagram · Twitter/X · Reddit · Facebook
        <br />
        Vimeo · Twitch · Bilibili · SoundCloud · Loom · Streamable
        <br />
        Pinterest · Tumblr · Threads · LinkedIn · 1000+ more
        <Telegram />
      </footer>
    </main>
  );
}

function Phone({ links }: { links: Links }) {
  const cards = usePage((s) => s.cards);
  return (
    <main className="flex flex-col gap-6 px-4 pt-6 pb-10">
      <div>
        <h1 className="m-0 font-serif text-[44px] leading-none font-normal tracking-[-0.03em]">
          Re<em className="text-accent italic">Clip</em>
        </h1>
        <p className="m-0 mt-2 text-[11px] tracking-[0.04em] text-muted uppercase">Free media downloader</p>
      </div>
      <div className="flex flex-col gap-2">
        <LinkBox links={links} phone />
        <p className="m-0 text-[11px] leading-[1.5] text-muted">One link per line, or separated by spaces.</p>
      </div>
      <div className="flex flex-col gap-2">
        <MediaControls phone />
      </div>
      <div className="flex flex-col gap-2">
        <ClipOptions phone />
        <FetchButton links={links} phone />
      </div>
      {cards.length > 0 && (
        <div className="flex flex-col gap-2.5">
          <DownloadAll phone />
          {cards.map((c) => (
            <PhoneCard key={c.key} c={c} />
          ))}
        </div>
      )}
      <History phone />
      <footer className="text-center text-[10px] leading-[1.9] tracking-[0.05em] text-muted uppercase">
        YouTube · TikTok · Instagram · X · Reddit
        <br />
        Facebook · Vimeo · Twitch · Bilibili · SoundCloud
        <br />
        and 1000+ more
        <Telegram phone />
      </footer>
    </main>
  );
}

/** The public page: a dedicated phone layout at ≤ 560 px, the desktop one (also used on tablets) above. */
export function App() {
  const phone = usePhone();
  const links = useLinks();
  return (
    <div className="min-h-screen bg-bg font-mono text-[13px] leading-[normal] text-fg">
      {phone ? <Phone links={links} /> : <Desktop links={links} />}
    </div>
  );
}
