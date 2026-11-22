import { useMemo, useState } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { demoLastFormatAtoms, demoShelves } from "../atoms/deck-shelf";
import {
  DEMO_FORMATS,
  type DemoDeck,
  type DemoFormat,
} from "../collection/data/demo-model";
import type { DemoMutationResult } from "../collection/data/demo-store";
import {
  legalityIn,
  useBuilderCatalogue,
  type BuilderCatalogue,
} from "../data/builder-catalogue";
import { faceCardOf } from "../data/deck-face";
import { readDeckIntake } from "../data/deck-text";
import type { BoxAppearance } from "@spellwake/deck-box";
import { useDemoAppearance } from "../cosmetics/demo-cosmetics";
import {
  failed,
  succeeded,
  type DeckBuilderSource,
  type DeckDraft,
  type DeckShelfSource,
  type FormatChoice,
  type Outcome,
  type PlayMode,
} from "./deck-sources";

/**
 * The local preview: decks kept in this browser and a small card set prepared
 * from the review fixtures. Nothing here reaches an account. A preview format
 * is known by its name, so its name is also its ID.
 */
const FORMATS: readonly FormatChoice[] = DEMO_FORMATS.map((name) => ({
  id: name,
  name,
}));
const formatNamed = (id: string): DemoFormat | undefined =>
  DEMO_FORMATS.find((name) => name === id);
const IMPORT_FAILED = "Couldn’t import deck.";
const COLOR_CODE: Record<string, string> = {
  White: "W",
  Blue: "U",
  Black: "B",
  Red: "R",
  Green: "G",
};
/**
 * The box a preview deck shows: its colours, and the official art of its
 * face card, which is the chosen one or else the first card added.
 */
function boxOf(
  deck: DemoDeck,
  catalogue: BuilderCatalogue | null | undefined,
): BoxAppearance {
  const art = (name: string): string | undefined =>
    catalogue?.find(name)?.artUrl;
  const face = faceCardOf(
    deck.appearance.faceCard,
    deck.entries.map((entry) => entry.name).filter((name) => art(name)),
  );
  return {
    colors: deck.appearance.colors,
    faceArt: (face && art(face)) ?? null,
  };
}
const outcome = <Value>(
  result: DemoMutationResult | null,
  value: (deck: DemoDeck) => Value,
  message: string,
): Outcome<Value> =>
  result?.status === "success"
    ? succeeded(value(result.deck))
    : failed(message);
export function useDemoDeckShelf(mode: PlayMode): DeckShelfSource {
  const { defaults } = useDemoAppearance();
  const { catalogue } = useBuilderCatalogue();
  const state = useAtomValue(demoShelves[mode].libraryAtom);
  const dispatch = useSetAtom(demoShelves[mode].actionAtom);
  const lastFormat = useAtomValue(demoLastFormatAtoms[mode]);
  const rememberFormat = useSetAtom(demoLastFormatAtoms[mode]);
  if (!state.library) return { status: "unavailable" };
  return {
    status: "ready",
    decks: state.library.decks.map((deck) => ({
      id: deck.id,
      name: deck.name,
      format: { id: deck.format, name: deck.format },
      appearance: boxOf(deck, catalogue),
    })),
    formats: FORMATS,
    lastFormatId: lastFormat,
    rememberFormat: (formatId) => {
      const format = formatNamed(formatId);
      if (format) rememberFormat(format);
    },
    createDeck: (formatId) => {
      const format = formatNamed(formatId);
      return Promise.resolve(
        format
          ? outcome(
              dispatch({
                type: "create",
                // A new deck starts with the saved defaults.
                input: {
                  name: "Untitled",
                  mode,
                  format,
                  entries: [],
                  cosmetics: defaults,
                },
              }),
              (deck) => ({ deckId: deck.id }),
              "Couldn’t create deck.",
            )
          : failed("Couldn’t create deck."),
      );
    },
    // The preview knows only its own small card set, so only those cards are found.
    importDeck: (formatId, text) => {
      const format = formatNamed(formatId);
      if (!format || !catalogue) return Promise.resolve(failed(IMPORT_FAILED));
      let lines;
      try {
        ({ lines } = readDeckIntake(text));
      } catch {
        return Promise.resolve(failed(IMPORT_FAILED));
      }
      const entries = lines.flatMap((line) => {
        const card = catalogue.find(line.name);
        return card
          ? [
              {
                id: crypto.randomUUID(),
                name: card.name,
                quantity: line.quantity,
                section: line.section,
              },
            ]
          : [];
      });
      return new Promise((resolve) =>
        // Long enough for the working notice to be read.
        window.setTimeout(
          () =>
            resolve(
              outcome(
                dispatch({
                  type: "create",
                  input: {
                    name: "Untitled",
                    mode,
                    format,
                    entries,
                    cosmetics: defaults,
                  },
                }),
                (deck) => ({ deckId: deck.id }),
                IMPORT_FAILED,
              ),
            ),
          600,
        ),
      );
    },
  };
}

/** One preview deck by ID, from whichever mode's shelf holds it. */
export function useDemoDeckBuilder(
  deckId: string | undefined,
): DeckBuilderSource {
  const open = useAtomValue(demoShelves.open.libraryAtom);
  const collection = useAtomValue(demoShelves.collection.libraryAtom);
  const writeOpen = useSetAtom(demoShelves.open.actionAtom);
  const writeCollection = useSetAtom(demoShelves.collection.actionAtom);
  const { catalogue, hasFailed } = useBuilderCatalogue();
  const appearance = useDemoAppearance();
  const [query, setQuery] = useState("");
  const [color, setColor] = useState("All");
  const [type, setType] = useState("All");
  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return (catalogue?.cards ?? [])
      .filter(
        (card) =>
          (needle === "" ||
            card.name.toLowerCase().includes(needle) ||
            card.rules.toLowerCase().includes(needle)) &&
          (color === "All" ||
            (color === "Colorless"
              ? card.colors.length === 0
              : card.colors.includes(COLOR_CODE[color] ?? ""))) &&
          (type === "All" || card.type.split(" — ")[0]?.includes(type)),
      )
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((facts) => ({ name: facts.name, facts }));
  }, [catalogue, query, color, type]);

  const deck = [open, collection]
    .flatMap((state) => state.library?.decks ?? [])
    .find((candidate) => candidate.id === deckId);
  const lines = useMemo(() => deck?.entries ?? [], [deck]);
  if (!deck) return { status: "missing" };
  const write = deck.mode === "open" ? writeOpen : writeCollection;
  const input = (draft: DeckDraft) => {
    const format = formatNamed(draft.formatId);
    return (
      format && {
        name: draft.name,
        format,
        // Preview rows are added by name, so none carries a card identity into preview storage.
        entries: draft.lines,
        mode: deck.mode,
        appearance: { ...deck.appearance, faceCard: draft.faceCard },
        ...(draft.appearance ? { cosmetics: draft.appearance } : {}),
        ...(deck.sourceText === undefined
          ? {}
          : { sourceText: deck.sourceText }),
      }
    );
  };
  const done = (): undefined => undefined;
  return {
    status: "ready",
    deck: {
      id: deck.id,
      name: deck.name,
      formatId: deck.format,
      mode: deck.mode,
      lines,
      appearance: deck.cosmetics ?? appearance.defaults,
      faceCard: deck.appearance.faceCard,
    },
    formats: FORMATS,
    appearanceOptions: appearance.options,
    canChooseFaceCard: true,
    showsOnBox: (name) => Boolean(catalogue?.find(name)?.artUrl),
    facts: (name) => catalogue?.find(name),
    legality: (name, format) => legalityIn(catalogue?.find(name), format.name),
    printingOf: (line) => catalogue?.find(line.name)?.printing ?? null,
    search: {
      query,
      setQuery,
      filters: { color, setColor, type, setType },
      state: hasFailed ? "failed" : catalogue ? "ready" : "loading",
      results,
    },
    hasCardsFailed: hasFailed,
    save: (draft) => {
      const next = input(draft);
      return Promise.resolve(
        next
          ? outcome(
              // A stale revision is refused, so a second window cannot overwrite this one.
              write({
                type: "update",
                id: deck.id,
                expectedRevision: deck.revision,
                input: next,
              }),
              done,
              "Couldn’t save deck.",
            )
          : failed("Couldn’t save deck."),
      );
    },
    duplicate: (draft) => {
      const next = input(draft);
      return Promise.resolve(
        next
          ? outcome(
              write({ type: "create", input: next }),
              done,
              "Couldn’t duplicate deck.",
            )
          : failed("Couldn’t duplicate deck."),
      );
    },
    remove: () =>
      Promise.resolve(
        outcome(
          write({
            type: "delete",
            id: deck.id,
            expectedRevision: deck.revision,
          }),
          done,
          "Couldn’t delete deck.",
        ),
      ),
  };
}
