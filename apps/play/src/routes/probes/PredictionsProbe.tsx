import { useEffect, useState, type ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { usePreviewPredictions } from "../../market/demo-predictions";
import type {
  Prediction,
  PredictionsSource,
  Quote,
  ReadyPredictions,
  SideQuote,
} from "../../market/prediction-sources";
import { PredictionsPage } from "../market/PredictionsPage";

/**
 * Development only: Predictions in states the preview cannot reach on its
 * own, for the stress checks and review captures. `?state=` names one. Each
 * starts from the preview and replaces only what that state changes.
 */
const noop = (): void => undefined;
const ready = (side: Partial<Record<"yes" | "no", SideQuote>>) => side;

export function PredictionsProbe(): ReactElement {
  const [params] = useSearchParams();
  const state = params.get("state") ?? "explore-loading";
  const preview = usePreviewPredictions();
  // Some states change their terms after a moment, as a fresh read would.
  const [tick, setTick] = useState(0);
  const quoteKey =
    preview.status === "ready" && preview.quote
      ? `${preview.quote.predictionId}:${preview.quote.amount}`
      : "";
  useEffect(() => {
    setTick(0);
    if (!quoteKey) return;
    const timer = window.setTimeout(() => setTick(1), 900);
    return () => window.clearTimeout(timer);
  }, [quoteKey]);

  if (state === "loading")
    return <PredictionsPage source={{ status: "loading" }} />;
  if (
    state === "signed-out" ||
    state === "unregistered" ||
    state === "unavailable"
  )
    return <PredictionsPage source={{ status: state }} />;
  if (preview.status !== "ready") return <PredictionsPage source={preview} />;

  const items = preview.explore.items;
  const withMine = (mine: Prediction["mine"]) =>
    items.map((prediction) => ({ ...prediction, mine }));
  const quoteWith = (
    sides: Partial<Record<"yes" | "no", SideQuote>>,
  ): Quote | null => (preview.quote ? { ...preview.quote, ...sides } : null);
  const many = Array.from({ length: 4 }, (_, round) =>
    items.map((prediction) => ({
      ...prediction,
      id: `${prediction.id}-${round}`,
    })),
  ).flat();
  const over = (overrides: Partial<ReadyPredictions>): PredictionsSource => ({
    ...preview,
    ...overrides,
  });
  const explore = preview.explore;
  const sources: Record<string, PredictionsSource> = {
    "explore-loading": over({
      explore: { ...explore, items: [], state: "loading" },
    }),
    "explore-failed": over({
      explore: { ...explore, items: [], state: "failed" },
      failure: "Could not load predictions",
      dismissFailure: noop,
    }),
    "explore-empty": over({ explore: { ...explore, items: [] } }),
    "explore-appending": over({
      explore: { ...explore, items: many, isAppending: true, hasMore: true },
    }),
    "refresh-failed": over({ failure: "Could not update predictions" }),
    "mine-loading": over({
      mine: {
        bets: { items: [], state: "loading" },
        created: { items: [], state: "loading" },
      },
    }),
    "mine-failed": over({
      mine: {
        bets: { items: [], state: "failed" },
        created: { items: [], state: "failed" },
      },
      failure: "Could not update predictions",
    }),
    "mine-partial": over({
      mine: { ...preview.mine, created: { items: [], state: "unavailable" } },
    }),
    "mine-empty": over({
      mine: {
        bets: { items: [], state: "ready" },
        created: { items: [], state: "ready" },
      },
    }),
    "mine-long": over({
      mine: {
        bets: {
          items: many.filter((p) => p.mine.kind === "bet"),
          state: "ready",
        },
        created: { items: many.filter((p) => p.authored), state: "ready" },
      },
    }),
    "result-unknown": over({
      explore: { ...explore, items: withMine({ kind: "unknown" }) },
    }),
    "result-unverified": over({
      explore: { ...explore, items: withMine({ kind: "unverified" }) },
    }),
    "bet-checking": over({
      quote: quoteWith({ yes: { kind: "checking" }, no: { kind: "checking" } }),
    }),
    "bet-failed": over({
      quote: quoteWith({ yes: { kind: "failed" }, no: { kind: "failed" } }),
    }),
    "bet-expired": over({
      quote: quoteWith({ yes: { kind: "expired" }, no: { kind: "expired" } }),
    }),
    "bet-none": over({
      quote: quoteWith({
        yes: { kind: "unavailable" },
        no: { kind: "unavailable" },
      }),
    }),
    "bet-side": over({
      quote: quoteWith(
        ready({
          yes: { kind: "unavailable" },
          no: preview.quote?.no ?? { kind: "checking" },
        }),
      ),
    }),
    "bet-too-much": over({
      quote: quoteWith({ yes: { kind: "too-much" }, no: { kind: "too-much" } }),
    }),
    // Terms that change after the side is armed: the Confirm is withdrawn.
    "bet-changed": over({
      quote:
        tick === 1 && preview.quote?.yes.kind === "ready"
          ? quoteWith({
              yes: {
                kind: "ready",
                win: preview.quote.yes.win - 100,
                revision: "changed",
              },
            })
          : preview.quote,
    }),
    "bet-pending": over({ betPending: true }),
    "bet-failure": over({
      placeBet: noop,
      commandFailure: "Couldn’t place bet.",
    }),
    "bet-closed": over({ betBlock: "closed" }),
    "bet-unavailable": over({ betBlock: "unavailable" }),
    "bet-account": over({ betBlock: "account" }),
    "bet-not-accepting": over({ betBlock: "not-accepting" }),
    "no-coins": over({ balance: 50 }),
    "create-checking": over({
      price: preview.price
        ? { printingId: preview.price.printingId, state: "checking" }
        : null,
    }),
    "create-unavailable": over({
      price: preview.price
        ? { printingId: preview.price.printingId, state: "unavailable" }
        : null,
    }),
    "create-price-failed": over({
      price: preview.price
        ? { printingId: preview.price.printingId, state: "failed" }
        : null,
    }),
    "create-pending": over({ publishPending: true }),
    "create-failed": over({
      publish: noop,
      commandFailure: "Couldn’t create prediction.",
    }),
    "picker-loading": over({
      cards: { ...preview.cards, items: [], state: "loading" },
    }),
    "picker-failed": over({
      cards: { ...preview.cards, items: [], state: "failed" },
    }),
    "picker-appending": over({
      cards: { ...preview.cards, isAppending: true, hasMore: true },
    }),
  };
  return <PredictionsPage source={sources[state] ?? preview} />;
}
