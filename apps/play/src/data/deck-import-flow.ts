import type { z } from "zod";
import type { deckEntrySchema } from "@spellwake/application-contracts";
import type { ApplicationApi } from "./application-api";
import {
  mergeDeckEntries,
  resolveDeckLines,
  type ResolvedDeckLine,
} from "./deck-resolution";
import { readDeckIntake, type DeckTextLine } from "./deck-text";

type Entry = z.infer<typeof deckEntrySchema>;
/** Total tries at looking cards up, the first included. */
const LOOKUP_ATTEMPTS = 3;

export type ImportEntries =
  | { status: "found"; entries: Entry[] }
  /** The catalogue could not be asked, which is not the same as a card being absent. */
  | { status: "unavailable" }
  /** The list is too long, or its cards together exceed a deck's limits. */
  | { status: "too-large" };

/**
 * The cards a deck list names that the catalogue knows, ready to import.
 *
 * Lines that cannot be read, and cards the catalogue does not know or cannot
 * tell apart, are left out without a word, as approved. A card whose named
 * printing is not found is kept without a printing rather than dropped. A
 * lookup that fails is retried, and still failing makes the whole import
 * fail: a card is never left out because the catalogue could not be asked.
 */
export async function importEntries(
  api: Pick<ApplicationApi, "cards" | "namedPrintings">,
  bearer: string,
  text: string,
  pause: () => Promise<void> = () => Promise.resolve(),
): Promise<ImportEntries> {
  let lines: DeckTextLine[];
  try {
    lines = readDeckIntake(text).lines;
  } catch {
    return { status: "too-large" };
  }
  const lookUp = async (
    wanted: readonly DeckTextLine[],
  ): Promise<ResolvedDeckLine[] | null> => {
    let resolved = await resolveDeckLines(api, bearer, wanted);
    for (
      let attempt = 1;
      attempt < LOOKUP_ATTEMPTS &&
      resolved.some((line) => line.state === "unavailable");
      attempt += 1
    ) {
      await pause();
      const retried = await resolveDeckLines(
        api,
        bearer,
        resolved
          .filter((line) => line.state === "unavailable")
          .map((line) => line.line),
      );
      let next = 0;
      resolved = resolved.map((line) =>
        line.state === "unavailable" ? (retried[next++] ?? line) : line,
      );
    }
    return resolved.some((line) => line.state === "unavailable")
      ? null
      : resolved;
  };
  const resolved = await lookUp(lines);
  if (!resolved) return { status: "unavailable" };
  // A named printing that is not found falls back to the card alone.
  const unprinted = resolved
    .filter((line) => line.state === "missing" && line.line.printingHint)
    .map((line) => ({ ...line.line, printingHint: null }));
  const fallback = unprinted.length ? await lookUp(unprinted) : [];
  if (!fallback) return { status: "unavailable" };
  const found = [...resolved, ...fallback].flatMap((line) =>
    line.state === "resolved"
      ? [
          {
            ...line.identity,
            quantity: line.line.quantity,
            section: line.line.section,
          },
        ]
      : [],
  );
  const merged = mergeDeckEntries(found);
  return merged.status === "success"
    ? { status: "found", entries: merged.entries }
    : { status: "too-large" };
}
