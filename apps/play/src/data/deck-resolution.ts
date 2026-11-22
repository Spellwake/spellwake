import type { z } from "zod";
import {
  deckEntrySchema,
  saveDeckSchema,
} from "@spellwake/application-contracts";
import type { ApplicationApi } from "./application-api";
import type { DeckTextLine } from "./deck-text";

export type DeckIdentity = { definitionId: string; printingId: string | null };
export type ResolvedDeckLine = { line: DeckTextLine } & (
  | { state: "resolved"; identity: DeckIdentity }
  | { state: "missing" | "ambiguous" | "unavailable" }
);
type Entry = z.infer<typeof deckEntrySchema>;

/** Resolve each exact name/hint once, bounded to four requests; never choose an ambiguous row. */
export async function resolveDeckLines(
  api: Pick<ApplicationApi, "cards" | "namedPrintings">,
  bearer: string,
  lines: readonly DeckTextLine[],
  signal?: AbortSignal,
): Promise<ResolvedDeckLine[]> {
  type Lookup =
    | Omit<Extract<ResolvedDeckLine, { state: "resolved" }>, "line">
    | { state: "missing" | "ambiguous" | "unavailable" };
  const key = (line: DeckTextLine) =>
    JSON.stringify([line.name, line.printingHint]);
  const pending = [
    ...new Map(lines.map((line) => [key(line), line])).entries(),
  ];
  const results = new Map<string, Lookup>();
  let cursor = 0;
  await Promise.all(
    Array.from({ length: Math.min(4, pending.length) }, async () => {
      while (cursor < pending.length) {
        const item = pending[cursor++];
        if (!item) continue;
        const [id, line] = item;
        if (signal?.aborted) {
          results.set(id, { state: "unavailable" });
          continue;
        }
        try {
          if (line.printingHint) {
            const result = await api.namedPrintings(
              bearer,
              line.name,
              {
                ...(line.printingHint.setCode
                  ? { setCode: line.printingHint.setCode }
                  : {}),
                ...(line.printingHint.collectorNumber
                  ? { collectorNumber: line.printingHint.collectorNumber }
                  : {}),
              },
              null,
              signal,
            );
            if (result.status !== "success")
              results.set(id, { state: "unavailable" });
            else {
              const first = result.data.items[0];
              results.set(
                id,
                !first
                  ? { state: "missing" }
                  : result.data.items.length !== 1 ||
                      result.data.nextCursor !== null
                    ? { state: "ambiguous" }
                    : {
                        state: "resolved",
                        identity: {
                          definitionId: first.definitionId,
                          printingId: first.id,
                        },
                      },
              );
            }
          } else {
            const result = await api.cards(
              bearer,
              line.name,
              true,
              null,
              signal,
            );
            if (result.status !== "success")
              results.set(id, { state: "unavailable" });
            else {
              const first = result.data.items[0];
              results.set(
                id,
                !first
                  ? { state: "missing" }
                  : result.data.items.length !== 1 ||
                      result.data.nextCursor !== null
                    ? { state: "ambiguous" }
                    : {
                        state: "resolved",
                        identity: { definitionId: first.id, printingId: null },
                      },
              );
            }
          }
        } catch {
          console.warn("deck_resolution code=lookup_unavailable");
          results.set(id, { state: "unavailable" });
        }
      }
    }),
  );
  return lines.map((line) => ({
    line,
    ...(results.get(key(line)) ?? { state: "unavailable" }),
  }));
}

/** Combine repeated canonical tuples exactly, rejecting invalid input or overflow atomically. */
export function mergeDeckEntries(
  entries: readonly Entry[],
):
  | { status: "success"; entries: Entry[] }
  | { status: "fail"; message: string } {
  const combined = new Map<string, Entry>();
  for (const input of entries) {
    const parsed = deckEntrySchema.safeParse(input);
    if (!parsed.success)
      return {
        status: "fail",
        message: "Fix the deck entries before importing.",
      };
    const entry = parsed.data;
    const key = JSON.stringify([
      entry.definitionId,
      entry.printingId,
      entry.section,
    ]);
    const prior = combined.get(key);
    combined.set(
      key,
      prior
        ? {
            ...entry,
            quantity: String(BigInt(prior.quantity) + BigInt(entry.quantity)),
          }
        : entry,
    );
  }
  const result = saveDeckSchema.shape.entries.safeParse([...combined.values()]);
  return result.success
    ? { status: "success", entries: result.data }
    : {
        status: "fail",
        message: "The combined deck exceeds its entry or quantity limit.",
      };
}

/** An import is all-or-nothing; unresolved input is never silently omitted. */
export function assembleDeckImport(lines: readonly ResolvedDeckLine[]) {
  if (!lines.length || lines.some((line) => line.state !== "resolved"))
    return {
      status: "fail",
      message: "Resolve every card line before importing.",
    } as const;
  return mergeDeckEntries(
    lines.flatMap((line) =>
      line.state === "resolved"
        ? [
            {
              ...line.identity,
              quantity: line.line.quantity,
              section: line.line.section,
            },
          ]
        : [],
    ),
  );
}
