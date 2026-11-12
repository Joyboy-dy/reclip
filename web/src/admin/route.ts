import { useCallback, useEffect, useState } from "react";

export type Route = "home" | "keys" | "password";

/** Maps a pathname to a page; unknown paths under /admin fall back to home. */
export function parseRoute(pathname: string): Route {
  const path = pathname.replace(/\/+$/, "");
  if (path === "/admin/keys") return "keys";
  if (path === "/admin/password") return "password";
  return "home";
}

export const routePath = (route: Route): string =>
  route === "keys" ? "/admin/keys" : route === "password" ? "/admin/password" : "/admin";

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
