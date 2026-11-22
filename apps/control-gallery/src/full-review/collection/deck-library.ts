import type { ScreenProps } from "../../review-types";
import {
  DEMO_FORMATS,
  demoDeckPresets,
} from "../../../../play/src/collection/data/demo-deck-presets";
import { createSessionDeckLibrary } from "../../../../play/src/collection/data/demo-store";
import {
  createDemoSeed,
  createStressDecks,
} from "../../../../play/src/collection/data/demo-seed";
import type {
  DemoDeck,
  DemoDeckEntry,
  DemoFormat,
  DemoLibrary,
} from "../../../../play/src/collection/data/demo-model";

export const formats = DEMO_FORMATS;
function shelfDeck(deck: DemoDeck) {
  return {
    id: deck.id,
    name: deck.name,
    format: deck.format,
    artIndex: demoDeckPresets.findIndex(
      (preset) => preset.artwork === deck.appearance.artwork,
    ),
  };
}
export type ShelfDeck = ReturnType<typeof shelfDeck>;
const seed = createDemoSeed().decks;
export const decks = seed.map(shelfDeck);
const numbered = createStressDecks(48).slice(18);
const long = {
  ...seed[3]!,
  id: "00000000-0000-4000-9001-000000000001",
  name: "Moonlit Council of the Silver Coast",
  format: "Commander" as const,
};
export const longDeck = shelfDeck(long);
const modeFor = (source: string) =>
  source === "DL-05" ? "open" : "collection";

const libraries = new Map<
  string,
  ReturnType<typeof createSessionDeckLibrary>
>();
function library(source: string, examples = false) {
  const mode = modeFor(source);
  const group = examples ? source : mode;
  let store = libraries.get(group);
  if (!store) {
    const initial = examples ? (source === "DL-08" ? numbered : [long]) : seed;
    store = createSessionDeckLibrary(
      "spellwake:review:decks:v1:" + group,
      (): DemoLibrary => ({
        version: 1,
        decks: initial.map((deck) => ({ ...deck, mode })),
      }),
    );
    libraries.set(group, store);
  }
  return store;
}
function owner(source: string, id: string) {
  return library(
    source,
    (source === "DL-06" && id === long.id) ||
      (source === "DL-08" && numbered.some((d) => d.id === id)),
  );
}
export function savedDeck(source: string, id: string) {
  return owner(source, id)
    .read()
    .library?.decks.find((deck) => deck.id === id);
}
export function shelfFor(source: string) {
  const normal = library(source).read().library?.decks ?? [];
  if (source === "DL-06" || source === "DL-08") {
    const examples = library(source, true).read().library?.decks ?? [];
    return (
      source === "DL-06"
        ? [...normal, ...examples]
        : [
            ...examples,
            ...normal.filter((d) => !seed.some((s) => s.id === d.id)),
          ]
    ).map(shelfDeck);
  }
  return normal.map(shelfDeck);
}
export function updateSavedDeck(
  source: string,
  deck: DemoDeck,
  input: Pick<DemoDeck, "name" | "format" | "entries">,
) {
  return owner(source, deck.id).write({
    type: "update",
    id: deck.id,
    expectedRevision: deck.revision,
    input: {
      ...input,
      mode: deck.mode,
      appearance: deck.appearance,
      ...(deck.sourceText === undefined ? {} : { sourceText: deck.sourceText }),
    },
  });
}
export type DeckLaunch = {
  source: string;
  deckId: string;
  query: string;
  formatFilter: string;
  scrollTop: number;
  target?: "edit" | "create";
};

export type ShelfContext = Pick<
  DeckLaunch,
  "source" | "query" | "formatFilter" | "scrollTop"
>;
export function readShelfContext(data: unknown): ShelfContext | undefined {
  if (!data || typeof data !== "object") return;
  const { source, query, formatFilter, scrollTop } =
    data as Partial<ShelfContext>;
  if (
    typeof source === "string" &&
    /^DL-0[1-8]$/.test(source) &&
    typeof query === "string" &&
    typeof formatFilter === "string" &&
    typeof scrollTop === "number" &&
    Number.isFinite(scrollTop)
  )
    return { source, query, formatFilter, scrollTop };
}
export function openImport(go: ScreenProps["go"], origin: ShelfContext) {
  const history = window.parent.history;
  history.replaceState({ ...history.state, screenData: origin }, "");
  go("IMP-01", origin);
}
export function readDeckLaunch(data: unknown): DeckLaunch | undefined {
  if (!data || typeof data !== "object") return;
  const { source, deckId, query, formatFilter, scrollTop, target } =
    data as Partial<Record<keyof DeckLaunch, unknown>>;
  if (
    typeof source !== "string" ||
    !/^DL-0[1-8]$/.test(source) ||
    typeof deckId !== "string" ||
    typeof query !== "string" ||
    typeof formatFilter !== "string" ||
    typeof scrollTop !== "number" ||
    !Number.isFinite(scrollTop)
  )
    return;
  return {
    source,
    deckId: deckId === "review-moonlit-council" ? long.id : deckId,
    query,
    formatFilter,
    scrollTop,
    ...(target === "edit" || target === "create" ? { target } : {}),
  };
}
function navigateDeck(go: ScreenProps["go"], launch: DeckLaunch) {
  const history = window.parent.history;
  history.replaceState({ ...history.state, screenData: launch }, "");
  go(launch.target ? "DB-01" : "L-01-host", launch);
}
export const playDeck = (go: ScreenProps["go"], launch: DeckLaunch) =>
  navigateDeck(go, launch);
export const editDeck = (go: ScreenProps["go"], launch: DeckLaunch) =>
  navigateDeck(go, { ...launch, target: "edit" });
function parseFormat(value: string | null): DemoFormat | undefined {
  return formats.find((f) => f === value);
}
export function rememberFormat(source: string, format: DemoFormat) {
  try {
    window.sessionStorage.setItem(
      "spellwake:review:last-format:" + modeFor(source),
      format,
    );
  } catch {
    /* This preference is optional. */
  }
}
export function createDeck(
  go: ScreenProps["go"],
  origin: ShelfContext,
  entries?: DemoDeckEntry[],
) {
  let format = parseFormat(origin.formatFilter);
  if (!format) {
    try {
      format = parseFormat(
        window.sessionStorage.getItem(
          "spellwake:review:last-format:" + modeFor(origin.source),
        ),
      );
    } catch {
      /* Use the first-use format. */
    }
  }
  const result = library(origin.source).write({
    type: "create",
    input: {
      name: "Untitled",
      mode: modeFor(origin.source),
      format: format ?? "Standard",
      entries: entries ?? [],
    },
  });
  if (result?.status !== "success") return false;
  rememberFormat(origin.source, result.deck.format);
  const launch = {
    ...origin,
    deckId: result.deck.id,
    target: "create",
  } as const;
  if (entries !== undefined) go("DB-01", launch);
  else navigateDeck(go, launch);
  return true;
}
