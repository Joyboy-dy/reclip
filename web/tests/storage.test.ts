import { afterEach, expect, test } from "bun:test";
import { loadDraft, saveDraft } from "../src/storage";

const orig = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
afterEach(() => {
  if (orig) Object.defineProperty(globalThis, "localStorage", orig);
  else delete (globalThis as any).localStorage;
});

test("draft round-trips", () => {
  const m = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) },
  });
  expect(loadDraft()).toBe("");
  saveDraft("https://a\nhttps://b");
  expect(m.get("reclip:draft")).toBe("https://a\nhttps://b");
  expect(loadDraft()).toBe("https://a\nhttps://b");
});

test("a throwing localStorage never throws", () => {
  const boom = () => { throw new Error("denied"); };
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: boom, setItem: boom } });
  expect(loadDraft()).toBe("");
  expect(() => saveDraft("x")).not.toThrow();
});
