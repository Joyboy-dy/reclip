import { useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { ApiFailure } from "@/api/client";
import type { AdminMe } from "@/api/types.gen";
import { TooltipProvider } from "@/components/ui/tooltip";
import { initNonce } from "@/lib/nonce";
import { getMe, logout } from "./api";
import { FirstLoginPage, LoginPage } from "./auth";
import { useLayout } from "./breakpoint";
import { KeysPage } from "./keys-page";
import { PasswordPage } from "./password-page";
import { useRoute } from "./route";
import { Shell } from "./shell";

// Must run before the first render so Radix style tags carry the CSP nonce.
initNonce();

type Session = { status: "loading" } | { status: "anonymous" } | { status: "in"; me: AdminMe; signInPassword: string };

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

  const signedOut = useCallback(() => setSession({ status: "anonymous" }), []);

  async function signOut() {
    try {
      await logout();
    } finally {
      signedOut();
      replace("home");
    }
  }

  if (session.status === "loading") return null;

  if (session.status === "anonymous") {
    return (
      <LoginPage
        onSignedIn={(me, password) => {
          setSession({ status: "in", me, signInPassword: password });
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
        onDone={() => {
          setSession({ status: "in", me: { ...me, must_change_password: false }, signInPassword: "" });
          go("keys");
        }}
      />
    );
  }

  // "/admin" is just the entry point: signed-in users land on the key list.
  const page = route === "password" ? "password" : "keys";
  return (
    <Shell layout={layout} route={page} email={me.email} go={go} onSignOut={signOut}>
      {page === "password" ? <PasswordPage /> : <KeysPage layout={layout} onUnauthorized={signedOut} />}
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
