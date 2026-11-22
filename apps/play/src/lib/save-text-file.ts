import { isTauri } from "@tauri-apps/api/core";

/**
 * Hands the player a text file to keep, on whichever host the app is running.
 * In the native app the system save dialog picks the place and the file is
 * written there. On the web the browser's own download does it.
 */
export type SaveFileOutcome =
  /** The native app wrote the file where the player chose. */
  | "saved"
  /** The browser took the file. Its own download list shows what happens next. */
  | "started"
  | "cancelled"
  | "failed";

/** What a host needs to offer to save a file. Passed in so the flow is testable. */
export type FileHost =
  | {
      kind: "native";
      /** Opens the system save dialog. Null when the player closes it. */
      choosePlace: (suggestedName: string) => Promise<string | null>;
      write: (place: string, text: string) => Promise<void>;
    }
  | { kind: "browser"; download: (name: string, text: string) => void };

/**
 * A file name any system accepts: no path separators, reserved characters or
 * control characters, no leading dots, a sensible length, and never empty.
 */
export function textFileName(title: string, fallback: string): string {
  const safe = Array.from(title.normalize("NFC"))
    .filter(
      (character) =>
        !'\\/:*?"<>|'.includes(character) &&
        (character.codePointAt(0) ?? 0) >= 0x20 &&
        character !== "\u007f",
    )
    .join("")
    .trim()
    .replace(/^\.+/, "")
    .replace(/[. ]+$/, "");
  return `${Array.from(safe).slice(0, 100).join("").trim() || fallback}.txt`;
}

/** The host this app is running on. The native plugins load only inside the native app. */
export function currentFileHost(): FileHost {
  if (!isTauri())
    return {
      kind: "browser",
      download: (name, text) => {
        const url = URL.createObjectURL(
          new Blob([text], { type: "text/plain;charset=utf-8" }),
        );
        const link = document.createElement("a");
        link.href = url;
        link.download = name;
        link.click();
        // The download reads the blob after this task, so release it a moment later.
        window.setTimeout(() => URL.revokeObjectURL(url), 10_000);
      },
    };
  return {
    kind: "native",
    choosePlace: async (suggestedName) => {
      const { save } = await import("@tauri-apps/plugin-dialog");
      return save({
        defaultPath: suggestedName,
        filters: [{ name: "Text", extensions: ["txt"] }],
      });
    },
    write: async (place, text) => {
      const { writeTextFile } = await import("@tauri-apps/plugin-fs");
      await writeTextFile(place, text);
    },
  };
}

export async function saveTextFile(
  name: string,
  text: string,
  host: FileHost = currentFileHost(),
): Promise<SaveFileOutcome> {
  try {
    if (host.kind === "browser") {
      host.download(name, text);
      return "started";
    }
    const place = await host.choosePlace(name);
    if (place === null) return "cancelled";
    await host.write(place, text);
    return "saved";
  } catch {
    // The reason may name a private path, so it is not logged.
    console.warn("save_text_file code=unavailable");
    return "failed";
  }
}
