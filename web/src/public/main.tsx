import { createRoot } from "react-dom/client";
import { getConfig, startBootstrap } from "../api/client";
import { initNonce } from "@/lib/nonce";
import { App } from "./App";
import { restore } from "./downloads";
import { loadHistory } from "./history";
import { store } from "./store";

// Must run before the first render so Radix style tags carry the CSP nonce.
initNonce();

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(<App />);

// The config request goes first and alone: its response sets the visitor
// cookie that every later request (restore, history, downloads) must share.
startBootstrap(getConfig)
  .then((config) => store.set({ config }))
  .catch(() => {});
void restore();
void loadHistory();
