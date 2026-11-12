import { expect, test } from "bun:test";
import { getNonce } from "get-nonce";
import { initNonce, readNonce } from "../src/lib/nonce";

const docWith = (content: string | null) =>
  ({
    querySelector: (sel: string) =>
      content !== null && sel === 'meta[name="csp-nonce"]' ? { getAttribute: () => content } : null,
  }) as unknown as Pick<Document, "querySelector">;

test("reads the nonce from the csp-nonce meta", () => {
  expect(readNonce(docWith("abc123"))).toBe("abc123");
});

test("ignores a missing meta, an empty value and the unreplaced placeholder", () => {
  expect(readNonce(docWith(null))).toBeUndefined();
  expect(readNonce(docWith("  "))).toBeUndefined();
  expect(readNonce(docWith("{{CSP_NONCE}}"))).toBeUndefined();
});

test("initNonce hands the nonce to get-nonce", () => {
  expect(initNonce(docWith("n0nce"))).toBe("n0nce");
  expect(getNonce()).toBe("n0nce");
});
