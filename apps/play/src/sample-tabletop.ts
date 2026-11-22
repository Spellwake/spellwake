import { mountPlayArea } from "spellwake-renderer-benchmark/play-area";
import { SAMPLE_PLAY_SETUP } from "spellwake-renderer-benchmark/play-setup";
import { initializeRuntime } from "./lib/runtime";
import "./privacy-link.css";

const lifetime = new AbortController();
window.addEventListener(
  "pagehide",
  (event: PageTransitionEvent) => {
    if (!event.persisted) lifetime.abort();
  },
  { signal: lifetime.signal },
);
import.meta.hot?.dispose(() => lifetime.abort());

const privacyLink = document.createElement("a");
privacyLink.className = "privacy-link";
privacyLink.href = new URL(
  "privacy/",
  new URL(import.meta.env.BASE_URL, document.baseURI),
).href;
privacyLink.textContent = "Privacy";
document.body.append(privacyLink);
lifetime.signal.addEventListener("abort", () => privacyLink.remove(), {
  once: true,
});

async function start(): Promise<void> {
  await initializeRuntime();
  await mountPlayArea(document, SAMPLE_PLAY_SETUP, {
    assetBaseUrl: new URL(import.meta.env.BASE_URL, document.baseURI),
    playMode: "fixture-preview",
    signal: lifetime.signal,
  });
}

void start().catch((error: unknown) => {
  if (lifetime.signal.aborted) return;
  console.error("Spellwake tabletop failed to load.", error);
  const status = document.getElementById("play-status");
  if (status) {
    status.classList.remove("sr-only");
    status.textContent = "Unable to load the table. Reload to retry.";
  }
});
