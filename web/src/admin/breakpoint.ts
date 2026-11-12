import { useEffect, useState } from "react";

export type Layout = "phone" | "tablet" | "desktop";

const PHONE = "(max-width: 560px)";
const TABLET = "(max-width: 900px)";

export const layoutFor = (matches: (query: string) => boolean): Layout =>
  matches(PHONE) ? "phone" : matches(TABLET) ? "tablet" : "desktop";

/** Current layout bucket: phone <= 560 px, tablet <= 900 px, desktop above. Re-renders when it changes. */
export function useLayout(): Layout {
  const read = () => layoutFor((q) => matchMedia(q).matches);
  const [layout, setLayout] = useState<Layout>(read);
  useEffect(() => {
    const lists = [matchMedia(PHONE), matchMedia(TABLET)];
    const update = () => setLayout(read());
    for (const l of lists) l.addEventListener("change", update);
    return () => {
      for (const l of lists) l.removeEventListener("change", update);
    };
  }, []);
  return layout;
}
