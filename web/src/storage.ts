const KEY = "reclip:draft";

/** The unsent link-box text, or "" when storage is unavailable. */
export function loadDraft(): string {
  try {
    return localStorage.getItem(KEY) ?? "";
  } catch {
    return "";
  }
}

export function saveDraft(text: string): void {
  try {
    localStorage.setItem(KEY, text);
  } catch {
    // storage blocked or full: the draft is a convenience only
  }
}
