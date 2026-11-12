import { getNonce, setNonce } from "get-nonce";

const PLACEHOLDER = /^\{\{.*\}\}$/;

/** Reads the per-response CSP nonce from `<meta name="csp-nonce">`; undefined while still the unreplaced template placeholder. */
export function readNonce(doc: Pick<Document, "querySelector">): string | undefined {
  const content = doc.querySelector('meta[name="csp-nonce"]')?.getAttribute("content")?.trim();
  return content && !PLACEHOLDER.test(content) ? content : undefined;
}

/** Hands the nonce to get-nonce so Radix style tags carry it. Call before the first React render. */
export function initNonce(doc: Pick<Document, "querySelector"> = document): string | undefined {
  const nonce = readNonce(doc);
  if (nonce) setNonce(nonce);
  return nonce;
}

/** Value for the `nonce` prop of Radix parts that render their own style tag (Select.Viewport, ScrollArea.Viewport). */
export function styleNonce(): string | undefined {
  return getNonce() ?? undefined;
}
