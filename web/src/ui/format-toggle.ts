import type { MediaFormat } from "../api/types.gen";

/** Wire the MP4/MP3 pills; returns a getter for the current format. */
export function setupFormatToggle(): () => MediaFormat {
  let current: MediaFormat = "mp4";
  const pills = document.querySelectorAll<HTMLButtonElement>(".pill");
  pills.forEach((pill) => {
    pill.addEventListener("click", () => {
      pills.forEach((p) => p.classList.remove("active"));
      pill.classList.add("active");
      current = pill.dataset.format as MediaFormat;
    });
  });
  return () => current;
}
