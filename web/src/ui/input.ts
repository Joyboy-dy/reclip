import { loadDraft, saveDraft } from "../storage";
import { parseUrls } from "../urls";

/** Wire the textarea (Enter submits, Shift+Enter newline) and the Fetch button. */
export function setupInput(onSubmit: () => void): {
  getUrls: () => string[];
  setBusy: (busy: boolean) => void;
  clearDraft: () => void;
} {
  const area = document.getElementById("urls") as HTMLTextAreaElement;
  const btn = document.getElementById("goBtn") as HTMLButtonElement;

  area.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      onSubmit();
    }
  });
  btn.addEventListener("click", onSubmit);
  area.value = loadDraft();
  area.addEventListener("input", () => saveDraft(area.value));

  return {
    getUrls: () => parseUrls(area.value),
    clearDraft: () => saveDraft(""),
    setBusy: (busy) => {
      btn.disabled = busy;
      btn.textContent = busy ? "Loading..." : "Fetch";
    },
  };
}
