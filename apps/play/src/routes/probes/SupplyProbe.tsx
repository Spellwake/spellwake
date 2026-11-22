import { useCallback, useEffect, useState, type ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { previewTerms } from "../../market/demo-supply";
import type {
  Ballot,
  BallotCandidate,
  QuoteTerms,
  ReadySupply,
  SupplySource,
  SupplyTile,
} from "../../market/supply-sources";
import { SupplyPage } from "../market/SupplyPage";

/**
 * Development only: Supply in states the preview cannot reach on its own,
 * for the stress checks and review captures. `?state=` names one.
 */
const noop = (): void => undefined;
const fdnItem = {
  id: "probe-fdn",
  setCode: "FDN",
  setName: "Foundations",
  product: "Play Booster",
  stock: 420,
  base: 1000,
  step: 3,
  packId: "preview-pack-fdn",
};
const tile = (
  id: string,
  code: string,
  setName: string,
  product: string,
  quote: QuoteTerms | null,
  quoteState: "checking" | "ready" | "failed" = "ready",
): SupplyTile => ({
  kind: "product",
  id,
  setId: `probe-set-${code}`,
  setName,
  setCode: code,
  product,
  fullName: `${setName} ${product}`,
  quote,
  quoteState,
});
const terms = (
  remaining: number,
  total: number | null,
  quantity = 1,
): QuoteTerms => ({
  quantity,
  remaining,
  total,
  policyId: "probe-policy",
  revision: "1",
  totalText: total === null ? null : String(total),
});
const CANDIDATES: BallotCandidate[] = [
  ["FDN", "Foundations"],
  ["MH3", "Modern Horizons 3"],
  ["BLB", "Bloomburrow"],
  ["DSK", "Duskmourn"],
  ["DMR", "Dominaria Remastered"],
  ["ISD", "Innistrad"],
  ["KTK", "Khans of Tarkir"],
  ["KHM", "Kaldheim"],
  ["ELD", "Throne of Eldraine"],
  ["THB", "Theros Beyond Death"],
  ["BRO", "The Brothers’ War"],
].map(([code, name]) => ({ setId: `set-${code}`, code: code!, name: name! }));
const openBallot = (
  overrides: Partial<Extract<Ballot, { status: "ready" }>> = {},
): Ballot => ({
  status: "ready",
  closes: "Dec 25",
  nextDrop: "Jan 1",
  isOpen: true,
  canVote: true,
  qualifies: true,
  candidates: CANDIDATES,
  saved: ["set-FDN", "set-MH3"],
  results: null,
  readFailed: false,
  save: null,
  ...overrides,
});

export function SupplyProbe(): ReactElement {
  const [params] = useSearchParams();
  const state = params.get("state") ?? "checking";
  const [target, setTarget] = useState<{
    tileId: string;
    quantity: number;
  } | null>(null);
  const [tick, setTick] = useState(0);
  const quoteFor = useCallback(
    (tileId: string | null, quantity: number) =>
      setTarget((current) =>
        tileId === null
          ? null
          : current?.tileId === tileId && current.quantity === quantity
            ? current
            : { tileId, quantity },
      ),
    [],
  );
  // Some states change their quote after a moment, as a live read would.
  useEffect(() => {
    setTick(0);
    if (!target) return;
    const timer = window.setTimeout(() => setTick(1), 600);
    return () => window.clearTimeout(timer);
  }, [target]);

  const tiles: SupplyTile[] = [
    tile("probe-fdn", "FDN", "Foundations", "Play Booster", terms(420, 1000)),
    tile(
      "probe-fdn-collector",
      "FDN",
      "Foundations",
      "Collector Booster",
      terms(40, 3000),
    ),
    tile(
      "probe-mh3",
      "MH3",
      "Modern Horizons 3",
      "Play Booster",
      state === "checking" ? null : terms(280, 1400),
      state === "checking" ? "checking" : "ready",
    ),
    tile("probe-dsk", "DSK", "Duskmourn", "Play Booster", terms(0, null)),
    {
      kind: "set",
      id: "probe-p15a",
      setName: "15th Anniversary Cards",
      setCode: "P15A",
    },
  ];
  const quantity = target?.quantity ?? 1;
  const live = previewTerms(fdnItem, 0, quantity);
  const quote: ReadySupply["quote"] = !target
    ? null
    : state === "buy-quote-loading"
      ? { ...target, state: "loading", terms: null }
      : state === "buy-quote-failed" && tick === 1
        ? { ...target, state: "failed", terms: null }
        : state === "buy-changed" && tick === 1
          ? {
              ...target,
              state: "ready",
              terms: {
                ...live,
                total: (live.total ?? 0) + 5,
                revision: "2",
                remaining: 419,
              },
            }
          : state === "buy-short"
            ? {
                ...target,
                state: "ready",
                terms: terms(
                  2,
                  quantity <= 2 ? 1000 * quantity + 3 : null,
                  quantity,
                ),
              }
            : { ...target, state: "ready", terms: live };
  const held = (stage: NonNullable<ReadySupply["purchase"]>["stage"]) => ({
    tileId: "probe-fdn",
    quantity: 1,
    terms: terms(420, 1000),
    stage,
  });
  const ready = (overrides: Partial<ReadySupply>): SupplySource => ({
    status: "ready",
    tiles,
    isAppending: state === "appending",
    isUnread: false,
    failure: null,
    dismissFailure: noop,
    balance: state === "buy-no-coins" ? 500 : 8000,
    canBuy: true,
    quote,
    quoteFor,
    purchase: null,
    buy: noop,
    checkPurchase: noop,
    dismissPurchase: noop,
    ballot: openBallot(),
    saveBallot: noop,
    checkBallot: noop,
    dismissBallot: noop,
    completed: null,
    dismissCompleted: noop,
    ...overrides,
  });
  const sources: Record<string, SupplySource> = {
    loading: { status: "loading" },
    "read-failed": ready({
      tiles: [],
      isUnread: true,
      failure: "Couldn’t load sets.",
    }),
    empty: ready({ tiles: [] }),
    "buy-pending": ready({ purchase: held({ kind: "pending" }) }),
    "buy-unconfirmed": ready({
      purchase: held({ kind: "unconfirmed", isChecking: false }),
    }),
    "buy-rejected": ready({
      purchase: held({
        kind: "rejected",
        message: "Product no longer available.",
      }),
    }),
    "vote-loading": ready({ ballot: { status: "loading" } }),
    "vote-none": ready({ ballot: { status: "none" } }),
    "vote-unavailable": ready({ ballot: { status: "unavailable" } }),
    "vote-not-eligible": ready({
      ballot: openBallot({ canVote: false, qualifies: false, saved: [] }),
    }),
    "vote-read-failed": ready({
      ballot: openBallot({ canVote: false, readFailed: true }),
    }),
    "vote-limit": ready({
      ballot: openBallot({
        saved: CANDIDATES.slice(0, 10).map((c) => c.setId),
      }),
    }),
    "vote-pending": ready({
      ballot: openBallot({ save: { kind: "pending" } }),
    }),
    "vote-unconfirmed": ready({
      ballot: openBallot({ save: { kind: "unconfirmed", isChecking: false } }),
    }),
    "vote-closed": ready({
      ballot: openBallot({ isOpen: false, canVote: false }),
    }),
    "vote-results": ready({
      ballot: openBallot({
        isOpen: false,
        canVote: false,
        results: [
          { name: "Foundations", votes: 320, packs: 1200 },
          { name: "Modern Horizons 3", votes: 290, packs: 1200 },
          { name: "Bloomburrow", votes: 250, packs: 1200 },
          { name: "Duskmourn", votes: 210, packs: 1200 },
          { name: "Dominaria Remastered", votes: 190, packs: 1200 },
        ],
      }),
    }),
    "no-ballot": ready({ ballot: null }),
  };
  return <SupplyPage source={sources[state] ?? ready({})} />;
}
