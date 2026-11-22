import { useCallback, useEffect, useState } from "react";

export type Route = "home" | "users" | "server" | "keys" | "password";

const PAGES: Exclude<Route, "home">[] = ["users", "server", "keys", "password"];

/** Maps a pathname to a page; unknown paths under /admin fall back to home. */
export function parseRoute(pathname: string): Route {
  const path = pathname.replace(/\/+$/, "");
  return PAGES.find((p) => path === `/admin/${p}`) ?? "home";
}

export const routePath = (route: Route): string => (route === "home" ? "/admin" : `/admin/${route}`);

/** History-API router: current route plus navigate/replace helpers. No dependency. */
export function useRoute(): { route: Route; go: (r: Route) => void; replace: (r: Route) => void } {
  const [route, setRoute] = useState<Route>(() => parseRoute(location.pathname));
  useEffect(() => {
    const onPop = () => setRoute(parseRoute(location.pathname));
    addEventListener("popstate", onPop);
    return () => removeEventListener("popstate", onPop);
  }, []);
  const go = useCallback((r: Route) => {
    history.pushState(null, "", routePath(r));
    setRoute(r);
  }, []);
  const replace = useCallback((r: Route) => {
    history.replaceState(null, "", routePath(r));
    setRoute(r);
  }, []);
  return { route, go, replace };
}
