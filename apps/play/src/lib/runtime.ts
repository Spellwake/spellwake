import { isTauri } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";

/** Identify the host and verify the native bridge without coupling the renderer to it. */
export async function initializeRuntime(): Promise<void> {
  document.documentElement.dataset.runtime = isTauri() ? "tauri" : "browser";
  if (!isTauri()) return;

  try {
    const version = await getVersion();
    document.documentElement.dataset.nativeVersion = version;
    console.info(`Spellwake native runtime ready (${version}).`);
  } catch (error: unknown) {
    document.documentElement.dataset.runtime = "tauri-error";
    console.error("Spellwake native runtime initialization failed.", error);
  }
}
