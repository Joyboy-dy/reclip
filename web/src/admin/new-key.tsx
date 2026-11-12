import { Plus } from "lucide-react";
import { useState, type FormEvent, type ReactNode } from "react";
import type { CreatedKey, Scope } from "@/api/types.gen";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { adminErrorText, createKey } from "./api";
import type { Layout } from "./breakpoint";
import { DEFAULT_NEW_KEY, EXPIRY_CHOICES, SCOPE_CHOICES, buildCreateRequest, type NewKeyForm } from "./lib";
import { Modal, ModalDescription, ModalHeader, ModalTitle } from "./modal";
import { CheckRow, ErrorLine, Field, TextInput } from "./parts";

const LIMITS: { key: "probes" | "jobs" | "active"; label: string; phoneLabel: string; placeholder: string }[] = [
  { key: "probes", label: "Analyses per min", phoneLabel: "Analyses per minute", placeholder: "120" },
  { key: "jobs", label: "Downloads per hour", phoneLabel: "Downloads per hour", placeholder: "200" },
  { key: "active", label: "Active at once", phoneLabel: "Active at once", placeholder: "100" },
];

/** Bordered list container used by the phone sheet (rights and limits rows). */
function RowList({ children }: { children: ReactNode }) {
  return (
    <div className="overflow-hidden rounded-[14px] border-[1.5px] border-line bg-card [&>*]:border-b [&>*]:border-line-soft [&>*:last-child]:border-b-0">
      {children}
    </div>
  );
}

function ExpiresSelect({ value, onChange, phone }: { value: string; onChange: (v: string) => void; phone: boolean }) {
  return (
    <Field id="key-expires" label="Expires">
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger id="key-expires" wide className={phone ? "h-12" : undefined}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {EXPIRY_CHOICES.map((c) => (
            <SelectItem key={c.value} value={c.value}>
              {c.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  );
}

function FormBody({ form, set, phone }: { form: NewKeyForm; set: (patch: Partial<NewKeyForm>) => void; phone: boolean }) {
  const toggle = (scope: Scope, on: boolean) =>
    set({ scopes: on ? [...form.scopes, scope] : form.scopes.filter((s) => s !== scope) });
  const limitInputs = LIMITS.map((l) => (
    <div key={l.key} className={phone ? "flex min-h-[52px] items-center justify-between gap-3 px-4 text-[13px]" : "flex flex-col gap-1.5"}>
      <Label htmlFor={`key-${l.key}`} className={phone ? "text-[13px] tracking-normal text-fg normal-case" : "tracking-normal normal-case"}>
        {phone ? l.phoneLabel : l.label}
      </Label>
      <Input
        id={`key-${l.key}`}
        inputMode="numeric"
        autoComplete="off"
        placeholder={l.placeholder}
        disabled={form.unlimited}
        value={form[l.key]}
        onChange={(e) => set({ [l.key]: e.target.value })}
        className={phone ? "h-9 w-[88px] rounded-lg px-2.5 py-0 text-right" : undefined}
      />
    </div>
  ));

  if (phone) {
    return (
      <>
        <Field id="key-name" label="Name">
          <TextInput id="key-name" autoComplete="off" value={form.name} onChange={(e) => set({ name: e.target.value })} />
        </Field>
        <div className="flex flex-col gap-2">
          <span className="text-[11px] tracking-[0.04em] text-muted uppercase">Rights</span>
          <RowList>
            {SCOPE_CHOICES.map((c) => (
              <CheckRow key={c.scope} id={`scope-${c.scope}`} className="min-h-[52px] px-4" checked={form.scopes.includes(c.scope)} onChange={(on) => toggle(c.scope, on)}>
                {c.phoneLabel ?? c.label}
              </CheckRow>
            ))}
          </RowList>
        </div>
        <ExpiresSelect value={form.expires} onChange={(expires) => set({ expires })} phone />
        <div className="flex flex-col gap-2">
          <span className="text-[11px] tracking-[0.04em] text-muted uppercase">Limits</span>
          <RowList>
            <CheckRow id="key-unlimited" className="min-h-[52px] px-4" checked={form.unlimited} onChange={(unlimited) => set({ unlimited })}>
              Unlimited
            </CheckRow>
            {limitInputs}
          </RowList>
          <p className="m-0 text-[11px] leading-normal text-muted">Leave a limit empty to use the default shown.</p>
        </div>
      </>
    );
  }

  return (
    <>
      <Field id="key-name" label="Name">
        <TextInput id="key-name" autoComplete="off" value={form.name} onChange={(e) => set({ name: e.target.value })} />
      </Field>
      <fieldset className="m-0 border-none p-0">
        <legend className="mb-2.5 p-0 text-[11px] tracking-[0.04em] text-muted uppercase">Rights</legend>
        <div className="grid grid-cols-2 gap-x-4 gap-y-2.5">
          {SCOPE_CHOICES.map((c) => (
            <CheckRow
              key={c.scope}
              id={`scope-${c.scope}`}
              className={c.scope === "delegate" ? "col-span-full" : undefined}
              checked={form.scopes.includes(c.scope)}
              onChange={(on) => toggle(c.scope, on)}
            >
              {c.label}
            </CheckRow>
          ))}
        </div>
      </fieldset>
      <div className="grid grid-cols-2 gap-4">
        <ExpiresSelect value={form.expires} onChange={(expires) => set({ expires })} phone={false} />
        <div className="flex flex-col gap-1.5">
          <span className="text-[11px] tracking-[0.04em] text-muted uppercase">Limits</span>
          <CheckRow id="key-unlimited" className="py-3" checked={form.unlimited} onChange={(unlimited) => set({ unlimited })}>
            Unlimited (my own key)
          </CheckRow>
        </div>
      </div>
      <div className="grid grid-cols-3 gap-3">{limitInputs}</div>
      <p className="m-0 text-[11px] leading-normal text-muted">Leave a limit empty to use the default shown.</p>
    </>
  );
}

export function NewKeyDialog({
  layout,
  open,
  onClose,
  onCreated,
}: {
  layout: Layout;
  open: boolean;
  onClose: () => void;
  onCreated: (created: CreatedKey) => void;
}) {
  const phone = layout === "phone";
  const [form, setForm] = useState<NewKeyForm>(DEFAULT_NEW_KEY);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<NewKeyForm>) => setForm((f) => ({ ...f, ...patch }));

  async function submit(e: FormEvent) {
    e.preventDefault();
    const built = buildCreateRequest(form);
    if ("error" in built) return setError(built.error);
    setBusy(true);
    setError(null);
    try {
      const created = await createKey(built.request);
      setForm(DEFAULT_NEW_KEY);
      onCreated(created);
    } catch (err) {
      setError(adminErrorText(err));
    } finally {
      setBusy(false);
    }
  }

  const close = () => {
    setForm(DEFAULT_NEW_KEY);
    setError(null);
    onClose();
  };

  return (
    <Modal layout={layout} open={open} onOpenChange={(o) => !o && close()}>
      <form onSubmit={submit} className={cn("flex flex-col", phone ? "gap-5" : "gap-[18px]")}>
        <ModalHeader>
          <ModalTitle>New API key</ModalTitle>
        </ModalHeader>
        <ModalDescription className="sr-only">Choose a name, rights, expiry and limits for the new key.</ModalDescription>
        <FormBody form={form} set={set} phone={phone} />
        {error && <ErrorLine>{error}</ErrorLine>}
        <div className={phone ? "grid grid-cols-2 gap-2" : "flex flex-wrap justify-end gap-2.5"}>
          <Button type="button" variant="secondary" size={phone ? "touch" : "md"} className={phone ? "h-12" : undefined} onClick={close}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" size={phone ? "touch" : "md"} className={phone ? "h-12" : undefined} disabled={busy}>
            Create key
          </Button>
        </div>
      </form>
    </Modal>
  );
}

export function NewKeyButton({ layout, onClick }: { layout: Layout; onClick: () => void }) {
  const phone = layout === "phone";
  return (
    <Button
      variant="primary"
      size="md"
      onClick={onClick}
      className={cn("shrink-0", phone && "h-12 w-full text-[13px] tracking-[0.05em]", layout === "tablet" && "h-11")}
    >
      <Plus className={phone ? "size-4" : "size-3.5"} strokeWidth={2.2} aria-hidden />
      New key
    </Button>
  );
}
