import { atomWithStorage } from "jotai/utils";
import { createDemoStore } from "../collection/data/demo-store";
import { createDemoSeed } from "../collection/data/demo-seed";
import type { DemoFormat, DemoLibrary } from "../collection/data/demo-model";
import type { PlayMode } from "../decks/deck-sources";

/**
 * Both preview shelves start from the same example decks. Each shelf gives
 * them its own IDs, because a deck is opened by ID alone and must resolve to
 * one shelf.
 */
const seedFor = (mode: PlayMode) => (): DemoLibrary => ({
  version: 1,
  decks: createDemoSeed().decks.map((deck) => ({
    ...deck,
    mode,
    id: mode === "open" ? deck.id : deck.id.replace("-8000-", "-8100-"),
  })),
});
/** Browser-local preview decks, one store per play mode. */
export const demoShelves = {
  open: createDemoStore(
    () => window.localStorage,
    "spellwake:decks:demo:v1:open",
    seedFor("open"),
  ),
  collection: createDemoStore(
    () => window.localStorage,
    "spellwake:decks:demo:v1:collection",
    seedFor("collection"),
  ),
} as const;
export const demoLastFormatAtoms = {
  open: atomWithStorage<DemoFormat | null>(
    "spellwake:decks:last-format:open",
    null,
  ),
  collection: atomWithStorage<DemoFormat | null>(
    "spellwake:decks:last-format:collection",
    null,
  ),
} as const;
/** The format last chosen on each signed-in shelf, kept apart from the preview's. */
export const accountLastFormatAtoms = {
  open: atomWithStorage<string | null>(
    "spellwake:decks:last-format-id:open",
    null,
  ),
  collection: atomWithStorage<string | null>(
    "spellwake:decks:last-format-id:collection",
    null,
  ),
} as const;
