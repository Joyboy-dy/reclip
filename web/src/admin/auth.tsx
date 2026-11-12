import { useState, type FormEvent } from "react";
import type { AdminMe } from "@/api/types.gen";
import { Button } from "@/components/ui/button";
import { adminErrorText, changePassword, isUnauthorized, login } from "./api";
import { validateNewPassword } from "./lib";
import { CenteredPage, ErrorLine, Field, Panel, StrengthMeter, TextInput } from "./parts";

const SUBMIT = "h-12 w-full text-[13px] tracking-[0.05em] min-[561px]:h-auto";

export function LoginPage({
  notice,
  onSignedIn,
}: {
  notice?: string;
  onSignedIn: (me: AdminMe, password: string) => void;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(notice ?? null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onSignedIn(await login({ email: email.trim(), password }), password);
    } catch (err) {
      setError(adminErrorText(err, "Wrong email or password."));
      setBusy(false);
    }
  }

  return (
    <CenteredPage>
      <form onSubmit={submit} className="flex w-full max-w-[420px] flex-col gap-[18px] max-[560px]:max-w-none max-[560px]:gap-6">
        <div>
          <h1 className="m-0 font-serif text-[52px] leading-none font-normal tracking-[-0.03em] max-[560px]:text-[44px]">
            Re<em className="text-accent">Clip</em>
          </h1>
          <p className="mt-2.5 mb-0 text-xs tracking-[0.04em] text-muted uppercase max-[560px]:text-[11px]">Admin</p>
        </div>
        <Panel>
          <Field id="admin-email" label="Email">
            <TextInput id="admin-email" type="email" autoComplete="username" required value={email} onChange={(e) => setEmail(e.target.value)} />
          </Field>
          <Field id="admin-password" label="Password">
            <TextInput id="admin-password" type="password" autoComplete="current-password" required value={password} onChange={(e) => setPassword(e.target.value)} />
          </Field>
          {error && <ErrorLine>{error}</ErrorLine>}
          <Button type="submit" variant="primary" size="lg" disabled={busy} className={SUBMIT}>
            Sign in
          </Button>
        </Panel>
        <p className="m-0 text-[11px] leading-normal text-muted">
          Only emails listed in RECLIP_ADMIN_EMAILS can sign in. First time? Use the default password from your server settings; you will choose your own right after.
        </p>
      </form>
    </CenteredPage>
  );
}

export function FirstLoginPage({
  me,
  initialPassword,
  onDone,
  onUnauthorized,
}: {
  me: AdminMe;
  initialPassword: string;
  onDone: () => void;
  onUnauthorized: () => void;
}) {
  // After a page reload the sign-in password is gone, so ask for the default one again.
  const [current, setCurrent] = useState(initialPassword);
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const invalid = validateNewPassword(next, confirm);
    if (invalid) return setError(invalid);
    setBusy(true);
    setError(null);
    try {
      await changePassword({ current, new: next, sign_out_others: true });
      onDone();
    } catch (err) {
      if (isUnauthorized(err)) return onUnauthorized();
      setError(adminErrorText(err));
      setBusy(false);
    }
  }

  return (
    <CenteredPage>
      <form onSubmit={submit} className="flex w-full max-w-[440px] flex-col gap-[18px] max-[560px]:max-w-none max-[560px]:gap-6">
        <div>
          <h1 className="m-0 font-serif text-[40px] leading-[1.1] font-normal tracking-[-0.02em] max-[560px]:text-[32px]">
            Choose your password
          </h1>
          <p className="mt-1.5 mb-0 text-xs leading-[1.6] text-muted">
            You signed in with the default password as {me.email}. Pick your own before going further. The default password stops working for this account.
          </p>
        </div>
        <Panel>
          {!initialPassword && (
            <Field id="first-current" label="Default password">
              <TextInput id="first-current" type="password" autoComplete="current-password" required value={current} onChange={(e) => setCurrent(e.target.value)} />
            </Field>
          )}
          <Field id="first-new" label="New password">
            <TextInput id="first-new" type="password" autoComplete="new-password" required value={next} onChange={(e) => setNext(e.target.value)} />
            <StrengthMeter password={next} />
          </Field>
          <Field id="first-confirm" label="Confirm password">
            <TextInput id="first-confirm" type="password" autoComplete="new-password" required value={confirm} onChange={(e) => setConfirm(e.target.value)} />
          </Field>
          {error && <ErrorLine>{error}</ErrorLine>}
          <Button type="submit" variant="primary" size="lg" disabled={busy} className={SUBMIT}>
            Save and continue
          </Button>
        </Panel>
      </form>
    </CenteredPage>
  );
}
