import { demoDeckPresets, demoDeckIdentity } from "./demo-deck-presets";
import { demoDeckSchema, type DemoDeck, type DemoLibrary } from "./demo-model";

const SEED_TIME = "2026-11-01T00:00:00.000Z";
/** Approved presentation examples have empty editable drafts, never invented card contents. */
export function createDemoSeed(): DemoLibrary {
  return {
    version: 1,
    decks: demoDeckPresets.map((seed, index) =>
      demoDeckSchema.parse({
        id: demoDeckIdentity(index).id,
        name: seed.name,
        format: seed.format,
        mode: "open",
        entries: [],
        // No face card: the seeds hold no cards to choose one from.
        appearance: { colors: seed.colors, faceCard: null },
        revision: 1,
        createdAt: SEED_TIME,
        updatedAt: SEED_TIME,
      }),
    ),
  };
}

/** Read-only stress-route examples. Callers must never persist this fixture automatically. */
export function createStressDecks(count = 160): DemoDeck[] {
  const seeds = createDemoSeed().decks;
  return Array.from(
    { length: Math.max(0, Math.min(2000, count)) },
    (_, index) => {
      const seed = seeds[index % seeds.length];
      if (!seed) throw new Error("Demo seed is missing.");
      const { id, name } = demoDeckIdentity(index, true);
      return { ...seed, id, name };
    },
  );
}
