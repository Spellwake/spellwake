import { useMemo } from "react";
import {
  cardKey,
  useBuilderCatalogue,
  type BuilderCard,
} from "../data/builder-catalogue";
import {
  CARD_COLORS,
  orderPrintings,
  type CardColor,
  type CardsSource,
  type OwnedCard,
  type OwnedPrinting,
} from "./card-sources";
import { useDemoCollection, type DemoHolding } from "./demo-collection";
import { cardAssetId, useDemoListingHolds } from "../market/demo-market-store";

/** Formats the preview catalogue records legality for, in the order the menu lists them. */
const FORMATS = [
  "Standard",
  "Pioneer",
  "Modern",
  "Legacy",
  "Vintage",
  "Commander",
  "Pauper",
];
/** Printed colour, read from the mana cost's symbols, hybrid halves included. */
export const costColors = (card: BuilderCard): CardColor[] =>
  CARD_COLORS.filter((color) =>
    card.cost.some((symbol) => symbol.split("/").includes(color)),
  );
/** Colour identity as the catalogue records it, with the cost's colours. */
export const identityOf = (card: BuilderCard): CardColor[] =>
  CARD_COLORS.filter(
    (color) => card.colors.includes(color) || costColors(card).includes(color),
  );

/** One owned card from the preview catalogue, or null when the catalogue lacks it. */
function ownedCard(card: BuilderCard, held: DemoHolding): OwnedCard | null {
  const id = `preview-card-${cardKey(card.name).replaceAll(/[^a-z0-9]+/g, "-")}`;
  const printing = (
    finish: "Nonfoil" | "Foil",
    copies: number,
  ): OwnedPrinting => ({
    id: `${id}-${finish.toLowerCase()}`,
    setCode: card.printing?.set ?? null,
    // The preview catalogue records set codes, not set names.
    setName: card.printing?.set ?? "Unknown set",
    number: card.printing?.number ?? null,
    finish,
    // Nor does it record rarity, so the preview offers no rarity filter.
    rarity: null,
    copies,
    faceUrl: card.faceUrl,
    backUrl: null,
  });
  const printings = orderPrintings(
    [
      held.nonfoil > 0 ? printing("Nonfoil", held.nonfoil) : null,
      held.foil > 0 ? printing("Foil", held.foil) : null,
    ].filter((entry): entry is OwnedPrinting => entry !== null),
  );
  if (printings.length === 0) return null;
  const colors = costColors(card);
  return {
    id,
    name: card.name,
    copies: held.nonfoil + held.foil,
    printings,
    type: card.type,
    rules: card.rules,
    manaValue: card.manaValue,
    colors,
    identity: identityOf(card),
    legality: card.legality,
  };
}

/** The preview collection: the preview catalogue's cards, counting only the copies usable now. */
export function useDemoCards(): CardsSource {
  const { catalogue, hasFailed } = useBuilderCatalogue();
  const [collection] = useDemoCollection();
  const holds = useDemoListingHolds();
  return useMemo((): CardsSource => {
    const facets = {
      formats: FORMATS,
      color: true,
      type: true,
      rarity: false,
      manaValue: true,
    };
    if (hasFailed)
      return {
        status: "ready",
        cards: [],
        facets,
        isAppending: false,
        failure: "Couldn’t load your cards.",
        isUnread: true,
      };
    if (!catalogue) return { status: "loading" };
    const cards = Object.entries(collection.cards).flatMap(([name, held]) => {
      const card = catalogue.find(name);
      // Only copies usable now are counted: those your listings hold are left out.
      const usable = {
        nonfoil: Math.max(
          0,
          held.nonfoil - (holds[cardAssetId(name, false)] ?? 0),
        ),
        foil: Math.max(0, held.foil - (holds[cardAssetId(name, true)] ?? 0)),
      };
      const owned = card ? ownedCard(card, usable) : null;
      return owned ? [owned] : [];
    });
    return {
      status: "ready",
      cards,
      facets,
      isAppending: false,
      failure: null,
      isUnread: false,
    };
  }, [catalogue, hasFailed, collection.cards, holds]);
}
