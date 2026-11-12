import { useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { ApiFailure } from "@/api/client";
import type { AdminMe } from "@/api/types.gen";
import { TooltipProvider } from "@/components/ui/tooltip";
import { initNonce } from "@/lib/nonce";
import { SESSION_EXPIRED, getMe, logout } from "./api";
import { FirstLoginPage, LoginPage } from "./auth";
import { useLayout } from "./breakpoint";
import { KeysPage } from "./keys-page";
import { PasswordPage } from "./password-page";
import { useRoute } from "./route";
import { Shell } from "./shell";

// Must run before the first render so Radix style tags carry the CSP nonce.
initNonce();

type Session = { status: "loading" } | { status: "anonymous"; notice?: string } | { status: "in"; me: AdminMe; signInPassword: string };

function App() {
  const layout = useLayout();
  const { route, go, replace } = useRoute();
  const [session, setSession] = useState<Session>({ status: "loading" });

  useEffect(() => {
    getMe().then(
      (me) => setSession({ status: "in", me, signInPassword: "" }),
      (err) => {
        // 401 is the normal "not signed in"; anything else also lands on the sign-in form, which reports errors itself.
        if (!(err instanceof ApiFailure)) console.error(err);
        setSession({ status: "anonymous" });
      },
    );
  }, []);

  // Any 401 from an admin call means the session is gone: back to sign-in with a notice.
  const expired = useCallback(() => setSession({ status: "anonymous", notice: SESSION_EXPIRED }), []);

  // "/admin" is only an entry point: signed-in users land on the key list.
  const signedIn = session.status === "in" && !session.me.must_change_password;
  useEffect(() => {
    if (signedIn && route === "home") replace("keys");
  }, [signedIn, route, replace]);

  async function signOut() {
    try {
      await logout();
    } catch (err) {
      console.error(err); // still sign out locally; the cookie expires on its own
    }
    setSession({ status: "anonymous" });
    replace("home");
  }

  if (session.status === "loading") return null;

  if (session.status === "anonymous") {
    return (
      <LoginPage
        notice={session.notice}
        onSignedIn={(me, password) => {
          // Keep the password only while it is needed for the forced first change.
          setSession({ status: "in", me, signInPassword: me.must_change_password ? password : "" });
          if (!me.must_change_password) go("keys");
        }}
      />
    );
  }

  const { me, signInPassword } = session;
  if (me.must_change_password) {
    return (
      <FirstLoginPage
        me={me}
        initialPassword={signInPassword}
        onUnauthorized={expired}
        onDone={() => {
          setSession({ status: "in", me: { ...me, must_change_password: false }, signInPassword: "" });
          go("keys");
        }}
      />
    );
  }

  const page = route === "password" ? "password" : "keys";
  return (
    <Shell layout={layout} route={page} email={me.email} go={go} onSignOut={signOut}>
      {page === "password" ? <PasswordPage onUnauthorized={expired} /> : <KeysPage layout={layout} onUnauthorized={expired} />}
    </Shell>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <TooltipProvider>
    <App />
  </TooltipProvider>,
);
