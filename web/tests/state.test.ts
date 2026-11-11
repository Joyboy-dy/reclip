import { expect, test } from "bun:test";
import { expandPlaylists } from "../src/state";
import type { ProbeResponse } from "../src/api/types.gen";

test("expandPlaylists replaces a playlist by its entries", async () => {
  const probe = async (u: string): Promise<ProbeResponse> =>
    u.includes("list=")
      ? { kind: "playlist", title: "p", entries: ["https://1", "https://2"] }
      : { kind: "video", title: "v", thumbnail: "", duration: 1, uploader: "", formats: [] };
  expect(await expandPlaylists(["https://x?list=1", "https://y"], probe)).toEqual(["https://1", "https://2", "https://y"]);
});
