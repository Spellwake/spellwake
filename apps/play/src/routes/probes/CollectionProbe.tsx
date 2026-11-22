import type { ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import type { CardsSource } from "../../collection/card-sources";
import { useDemoCards } from "../../collection/demo-cards";
import type {
  Opening,
  OwnedPack,
  PacksSource,
} from "../../collection/pack-sources";
import { CardsPage } from "../cards/CardsPage";
import { PacksPage } from "../packs/PacksPage";

/**
 * Development only: Cards and Packs in states the preview cannot reach on its
 * own, for the stress checks and review captures. `?state=` names one.
 */
const noop = (): void => undefined;
const facets = {
  formats: null,
  color: false,
  type: false,
  rarity: true,
  manaValue: false,
};
const pack = (setName: string, owned: number, held = 0): OwnedPack => ({
  id: `probe-${setName}`,
  setName,
  product: `${setName} Play Booster`,
  setCode: null,
  owned,
  held,
  available: owned - held,
});
const PACKS = [pack("Foundations", 3, 1), pack("Bloomburrow", 1, 1)];
const opening = (stage: Opening["stage"]): Opening => ({
  packId: "probe-Foundations",
  product: "Foundations Play Booster",
  stage,
  cards: null,
});
const packs = (
  overrides: Partial<Extract<PacksSource, { status: "ready" }>>,
): PacksSource => ({
  status: "ready",
  packs: PACKS,
  isAppending: false,
  isUnread: false,
  notice: null,
  dismissNotice: noop,
  canOpen: true,
  opening: null,
  open: noop,
  checkOpening: noop,
  closeOpening: noop,
  ...overrides,
});

function CardsProbe({ state }: { state: string }): ReactElement {
  const demo = useDemoCards();
  const ready = demo.status === "ready" ? demo : null;
  const source: CardsSource =
    state === "cards-loading"
      ? { status: "loading" }
      : state === "cards-empty"
        ? {
            status: "ready",
            cards: [],
            facets,
            isAppending: false,
            failure: null,
            isUnread: false,
          }
        : state === "cards-unavailable"
          ? {
              status: "ready",
              cards: [],
              facets,
              isAppending: false,
              failure: "Couldn’t load your cards.",
              isUnread: true,
            }
          : state === "cards-no-artwork"
            ? ready
              ? {
                  ...ready,
                  // The account's catalogue has no artwork: every face is a quiet frame.
                  cards: ready.cards.map((card) => ({
                    ...card,
                    printings: card.printings.map((p) => ({
                      ...p,
                      faceUrl: null,
                    })),
                  })),
                  facets,
                }
              : demo
            : ready
              ? state === "cards-appending"
                ? { ...ready, isAppending: true }
                : { ...ready, failure: "Couldn’t load more cards." }
              : demo;
  return <CardsPage source={source} />;
}

export function CollectionProbe(): ReactElement {
  const [params] = useSearchParams();
  const state = params.get("state") ?? "cards-loading";
  if (state.startsWith("cards-")) return <CardsProbe state={state} />;
  const source: PacksSource =
    state === "packs-loading"
      ? { status: "loading" }
      : state === "packs-unavailable"
        ? { status: "unavailable" }
        : state === "packs-empty"
          ? packs({ packs: [] })
          : state === "packs-rejected"
            ? packs({ notice: "This pack is no longer available." })
            : state === "packs-restricted"
              ? packs({
                  canOpen: false,
                  notice: "Pack opening is unavailable.",
                })
              : state === "opening-progress"
                ? packs({ opening: opening("opening") })
                : state === "opening-unconfirmed"
                  ? packs({ opening: opening("unconfirmed") })
                  : state === "opening-loading"
                    ? packs({ opening: opening("loading") })
                    : packs({});
  return <PacksPage source={source} />;
}
