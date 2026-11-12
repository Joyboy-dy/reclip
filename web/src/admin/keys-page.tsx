import { useCallback, useEffect, useState } from "react";
import { ApiFailure } from "@/api/client";
import type { ApiKeyInfo, CreatedKey } from "@/api/types.gen";
import { adminErrorText, listKeys } from "./api";
import type { Layout } from "./breakpoint";
import { KeyCreatedDialog } from "./key-created";
import { KeyCards, KeysTable } from "./key-views";
import { NewKeyButton, NewKeyDialog } from "./new-key";
import { ErrorLine, PageTitle } from "./parts";
import { RevokeDialog } from "./revoke";

const LEAD = "Keys let agents and the Telegram bot use the API. A key is shown once, when you create it.";

export function KeysPage({ layout, onUnauthorized }: { layout: Layout; onUnauthorized: () => void }) {
  const [keys, setKeys] = useState<ApiKeyInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<CreatedKey | null>(null);
  const [revoking, setRevoking] = useState<ApiKeyInfo | null>(null);

  const load = useCallback(async () => {
    try {
      setKeys(await listKeys());
      setError(null);
    } catch (err) {
      if (err instanceof ApiFailure && err.status === 401) return onUnauthorized();
      setError(adminErrorText(err));
    }
  }, [onUnauthorized]);

  useEffect(() => {
    void load();
  }, [load]);

  const now = new Date();
  const button = <NewKeyButton layout={layout} onClick={() => setCreating(true)} />;

  return (
    <>
      <div className={layout === "phone" ? "flex flex-col gap-4" : "flex items-end justify-between gap-6"}>
        <PageTitle title="API keys">{layout === "tablet" ? "Keys let agents and the Telegram bot use the API. A key is shown once." : LEAD}</PageTitle>
        {button}
      </div>

      {error && <ErrorLine>{error}</ErrorLine>}
      {keys === null && !error && <p className="m-0 text-xs text-muted">Loading keys…</p>}
      {keys?.length === 0 && <p className="m-0 text-xs text-muted">No keys yet. Create one to give an agent access.</p>}
      {keys && keys.length > 0 &&
        (layout === "desktop" ? (
          <>
            <KeysTable keys={keys} now={now} onRevoke={setRevoking} />
            <p className="m-0 text-[11px] leading-normal text-muted">
              The calendar marks keys with an expiry date; hover it for the date and days left. Orange: expires within 7 days.
            </p>
          </>
        ) : (
          <KeyCards keys={keys} now={now} layout={layout} onRevoke={setRevoking} />
        ))}

      <NewKeyDialog
        layout={layout}
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(c) => {
          setCreating(false);
          setCreated(c);
          void load();
        }}
      />
      <KeyCreatedDialog layout={layout} created={created} onClose={() => setCreated(null)} />
      <RevokeDialog
        layout={layout}
        target={revoking}
        onClose={() => setRevoking(null)}
        onRevoked={() => {
          setRevoking(null);
          void load();
        }}
      />
    </>
  );
}
