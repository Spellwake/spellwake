import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useBuilderCatalogue } from "../data/builder-catalogue";
import { useDemoCollection, type DemoCollection } from "./demo-collection";
import { useDemoListingHolds } from "../market/demo-market-store";
import type { Opening, PackCard, PacksSource } from "./pack-sources";

/** Cards in a preview pack, as in the approved walkthrough. */
const PACK_SIZE = 14;
/** How long the preview takes to open a pack and then to read its cards. */
const OPENING_MS = 1100;
const CONTENTS_MS = 500;

/** A small seeded generator, so one opening always gives the same cards. */
function seeded(seed: number): () => number {
  let state = seed >>> 0 || 1;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

/** Takes one pack away and adds its cards to the preview collection. */
function applyOpening(
  collection: DemoCollection,
  packId: string,
  names: readonly { name: string; foil: boolean }[],
): DemoCollection {
  const cards = { ...collection.cards };
  for (const { name, foil } of names) {
    const held = cards[name] ?? { nonfoil: 0, foil: 0 };
    cards[name] = foil
      ? { ...held, foil: held.foil + 1 }
      : { ...held, nonfoil: held.nonfoil + 1 };
  }
  return {
    ...collection,
    cards,
    packs: collection.packs
      .map((pack) =>
        pack.id === packId ? { ...pack, owned: pack.owned - 1 } : pack,
      )
      .filter((pack) => pack.owned > 0),
  };
}

/**
 * The local preview of Packs. Its packs are examples in the approved
 * walkthrough's names, drawn in code. Opening one always succeeds, after a
 * short wait, and gives fourteen cards from the preview catalogue, one of
 * them foil. Nothing reaches an account and nothing is bought.
 */
export function useDemoPacks(): PacksSource {
  const { catalogue, hasFailed } = useBuilderCatalogue();
  const [collection, update] = useDemoCollection();
  const holds = useDemoListingHolds();
  const [opening, setOpening] = useState<Opening | null>(null);
  const timers = useRef<number[]>([]);
  useEffect(
    () => () => {
      for (const timer of timers.current) window.clearTimeout(timer);
    },
    [],
  );
  const later = useCallback((ms: number, run: () => void) => {
    timers.current.push(window.setTimeout(run, ms));
  }, []);

  const open = useCallback(
    (packId: string) => {
      const pack = collection.packs.find(
        (candidate) => candidate.id === packId,
      );
      const listed = holds[packId] ?? 0;
      if (!pack || pack.owned - pack.held - listed < 1 || !catalogue) return;
      setOpening({
        packId,
        product: pack.product,
        stage: "opening",
        cards: null,
      });
      const random = seeded(Date.now());
      const drawn = Array.from({ length: PACK_SIZE }, (_, index) => {
        const card =
          catalogue.cards[Math.floor(random() * catalogue.cards.length)];
        return { card, foil: index === PACK_SIZE - 1 };
      }).flatMap(({ card, foil }) => (card ? [{ card, foil }] : []));
      later(OPENING_MS, () => {
        // Accepted: the cards are owned from here, revealed or not.
        update((current) =>
          applyOpening(
            current,
            packId,
            drawn.map(({ card, foil }) => ({ name: card.name, foil })),
          ),
        );
        setOpening((current) => current && { ...current, stage: "loading" });
        later(CONTENTS_MS, () => {
          const cards: PackCard[] = drawn.map(({ card, foil }, index) => ({
            key: `${index}`,
            name: card.name,
            faceUrl: card.faceUrl,
            finish: foil ? "Foil" : "Nonfoil",
          }));
          setOpening(
            (current) => current && { ...current, stage: "opened", cards },
          );
        });
      });
    },
    [collection.packs, catalogue, update, later, holds],
  );

  return useMemo((): PacksSource => {
    if (!catalogue && !hasFailed) return { status: "loading" };
    return {
      status: "ready",
      packs: collection.packs.map((pack) => ({
        id: pack.id,
        setName: pack.setName,
        product: pack.product,
        setCode: pack.setCode,
        owned: pack.owned,
        held: pack.held,
        // Packs your preview market listings hold are owned but cannot be opened.
        available: Math.max(0, pack.owned - pack.held - (holds[pack.id] ?? 0)),
      })),
      isAppending: false,
      isUnread: false,
      notice: null,
      dismissNotice: () => undefined,
      // The preview can only open packs once its catalogue is read.
      canOpen: catalogue !== null,
      opening,
      open,
      checkOpening: () => undefined,
      closeOpening: () => setOpening(null),
    };
  }, [catalogue, hasFailed, collection.packs, opening, open, holds]);
}
