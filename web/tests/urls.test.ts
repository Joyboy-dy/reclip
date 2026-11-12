import { expect, test } from "bun:test";
import { parseUrls } from "../src/urls";

test("parseUrls splits, trims, dedupes and drops non-links", () => {
  expect(parseUrls("https://a , https://b\nhttps://a hello ftp://x")).toEqual(["https://a", "https://b"]);
});
