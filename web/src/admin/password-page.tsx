import { useState, type FormEvent } from "react";
import { Button } from "@/components/ui/button";
import { adminErrorText, changePassword } from "./api";
import { validateNewPassword } from "./lib";
import { CheckRow, ErrorLine, Field, PageTitle, Panel, StrengthMeter, TextInput } from "./parts";

export function PasswordPage() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [signOutOthers, setSignOutOthers] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setSaved(false);
    const invalid = validateNewPassword(next, confirm, current);
    if (invalid) return setError(invalid);
    setBusy(true);
    setError(null);
    try {
      await changePassword({ current, new: next, sign_out_others: signOutOthers });
      setCurrent("");
      setNext("");
      setConfirm("");
      setSaved(true);
    } catch (err) {
      setError(adminErrorText(err, "The current password is not correct."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="flex w-full max-w-[460px] flex-col gap-4 max-[560px]:max-w-none">
      <PageTitle title="Change password">Only for your account. Other admins keep theirs.</PageTitle>
      <Panel>
        <Field id="pw-current" label="Current password">
          <TextInput id="pw-current" type="password" autoComplete="current-password" required value={current} onChange={(e) => setCurrent(e.target.value)} />
        </Field>
        <Field id="pw-new" label="New password">
          <TextInput id="pw-new" type="password" autoComplete="new-password" required placeholder="At least 12 characters" value={next} onChange={(e) => setNext(e.target.value)} />
          {next && <StrengthMeter password={next} />}
        </Field>
        <Field id="pw-confirm" label="Confirm new password">
          <TextInput id="pw-confirm" type="password" autoComplete="new-password" required value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        </Field>
        <CheckRow id="pw-others" checked={signOutOthers} onChange={setSignOutOthers}>
          Sign out my other sessions
        </CheckRow>
        {error && <ErrorLine>{error}</ErrorLine>}
        {saved && (
          <p role="status" className="m-0 text-[11px] text-[#2d7a46]">
            Password updated.
          </p>
        )}
        <Button type="submit" variant="primary" size="lg" disabled={busy} className="h-12 w-full min-[561px]:h-auto">
          Update password
        </Button>
      </Panel>
    </form>
  );
}
