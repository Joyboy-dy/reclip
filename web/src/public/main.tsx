import { createRoot } from "react-dom/client";
import { getConfig } from "../api/client";
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

void restore();
void loadHistory();
getConfig()
  .then((config) => store.set({ config }))
  .catch(() => {});
