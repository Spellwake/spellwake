import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { z } from "zod";
import {
  useBuilderCatalogue,
  type BuilderCard,
} from "../data/builder-catalogue";
import { coinsOf, useDemoCollection } from "../collection/demo-collection";
import {
  addDays,
  scheduleFrom,
  utcDay,
  type Completed,
  type Condition,
  type MyBet,
  type Outcome,
  type Prediction,
  type PredictionsSource,
  type PrintingLabel,
  type PriceCheck,
  type Quote,
  type Side,
  type SideQuote,
} from "./prediction-sources";

/**
 * The local preview of Market Predictions. Questions name cards from the
 * preview catalogue; bets and created questions persist in this browser, and
 * a bet's coins leave the shared preview wallet. A newly created question has
 * no opposing bets yet, so nothing can be bet on it, as the design requires.
 */
const QUOTE_MS = 350;
const COMMAND_MS = 800;

/** Cents a Yes contract costs; No costs the rest of 100. Null: no bets on that side. */
interface Liquidity {
  yes: number | null;
  no: number | null;
  /** The most coins either side can take in one bet. */
  limit: number;
}
interface Seed {
  id: string;
  card: string;
  finish?: "Nonfoil" | "Foil";
  targetUsd: string;
  condition: Condition;
  /** End date, days from today. */
  endsIn: number;
  state: Prediction["state"];
  outcome?: Outcome;
  evidence?: { daysBeforeEnd: number; usd: string };
  bet?: { side: Side; cost: number; win: number };
  authored?: boolean;
  liquidity?: Liquidity;
}
const SEEDS: Seed[] = [
  {
    id: "bolt",
    card: "Lightning Bolt",
    targetUsd: "3.50",
    condition: "reached_by",
    endsIn: 70,
    state: "open",
    liquidity: { yes: 40, no: 60, limit: 4000 },
  },
  {
    id: "guide",
    card: "Goblin Guide",
    targetUsd: "4",
    condition: "reached_by",
    endsIn: 70,
    state: "open",
    liquidity: { yes: 25, no: null, limit: 3000 },
  },
  {
    id: "vial",
    card: "Aether Vial",
    targetUsd: "2",
    condition: "reached_by",
    endsIn: 98,
    state: "open",
    authored: true,
    liquidity: { yes: 55, no: 45, limit: 2500 },
  },
  {
    id: "krenko",
    card: "Krenko, Mob Boss",
    finish: "Foil",
    targetUsd: "12",
    condition: "at_deadline",
    endsIn: 120,
    state: "open",
    bet: { side: "yes", cost: 200, win: 500 },
    liquidity: { yes: 40, no: 60, limit: 2000 },
  },
  {
    id: "marshal",
    card: "Mogg War Marshal",
    targetUsd: "0.75",
    condition: "reached_by",
    endsIn: 60,
    state: "open",
    authored: true,
    liquidity: { yes: null, no: null, limit: 0 },
  },
  {
    id: "loyalist",
    card: "Legion Loyalist",
    targetUsd: "1.25",
    condition: "reached_by",
    endsIn: 3,
    state: "closed",
    bet: { side: "no", cost: 300, win: 500 },
  },
  {
    id: "bushwhacker",
    card: "Goblin Bushwhacker",
    targetUsd: "1.50",
    condition: "reached_by",
    endsIn: -12,
    state: "resolved",
    outcome: "yes",
    evidence: { daysBeforeEnd: 2, usd: "1.62" },
    bet: { side: "yes", cost: 200, win: 500 },
  },
  {
    id: "strike",
    card: "Lightning Strike",
    targetUsd: "0.50",
    condition: "at_deadline",
    endsIn: -20,
    state: "resolved",
    outcome: "no",
    evidence: { daysBeforeEnd: 0, usd: "0.38" },
    bet: { side: "yes", cost: 120, win: 300 },
  },
  {
    id: "shock",
    card: "Shock",
    targetUsd: "0.40",
    condition: "reached_by",
    endsIn: -30,
    state: "resolved",
    outcome: "void",
    bet: { side: "no", cost: 200, win: 400 },
    authored: true,
  },
  {
    id: "matron",
    card: "Goblin Matron",
    targetUsd: "2.25",
    condition: "reached_by",
    endsIn: -8,
    state: "resolved",
    outcome: "yes",
    evidence: { daysBeforeEnd: 5, usd: "2.31" },
  },
  {
    id: "chieftain",
    card: "Goblin Chieftain",
    targetUsd: "1",
    condition: "at_deadline",
    endsIn: 2,
    state: "closed",
  },
];

const storeSchema = z.object({
  bets: z.record(
    z.string(),
    z.object({
      side: z.enum(["yes", "no"]),
      cost: z.number().int().positive(),
      win: z.number().int().positive(),
    }),
  ),
  created: z.array(
    z.object({
      id: z.string(),
      printingId: z.string(),
      name: z.string(),
      setCode: z.string().nullable(),
      finish: z.string(),
      faceUrl: z.string().nullable(),
      targetUsd: z.string(),
      condition: z.enum(["reached_by", "at_deadline"]),
      deadline: z.string(),
      closesAt: z.string(),
    }),
  ),
});
type DemoPredictions = z.infer<typeof storeSchema>;
const EMPTY: DemoPredictions = { bets: {}, created: [] };
const storage = {
  getItem: (key: string, initial: DemoPredictions): DemoPredictions => {
    try {
      const raw = window.localStorage.getItem(key);
      const parsed = raw ? storeSchema.safeParse(JSON.parse(raw)) : null;
      return parsed?.success ? parsed.data : initial;
    } catch {
      return initial;
    }
  },
  setItem: (key: string, value: DemoPredictions): void => {
    try {
      window.localStorage.setItem(key, JSON.stringify(value));
    } catch {
      // A private window may refuse storage. The preview then forgets on reload.
    }
  },
  removeItem: (key: string): void => {
    try {
      window.localStorage.removeItem(key);
    } catch {
      // As above.
    }
  },
};
export const DEMO_PREDICTIONS_KEY = "spellwake:predictions:demo:v1";
const predictionsAtom = atomWithStorage<DemoPredictions>(
  DEMO_PREDICTIONS_KEY,
  EMPTY,
  storage,
  { getOnInit: true },
);

const printingOf = (
  card: BuilderCard,
  finish: "Nonfoil" | "Foil",
): PrintingLabel => ({
  printingId: `preview-printing-${card.name.toLowerCase().replaceAll(/[^a-z0-9]+/g, "-")}-${finish.toLowerCase()}`,
  name: card.name,
  setCode: card.printing?.set ?? null,
  finish,
  faceUrl: card.faceUrl,
});

/** A side's terms: whole coins in, the fixed 100 per contract back, capped by what is on offer. */
export function previewSideQuote(
  liquidity: Liquidity | undefined,
  side: Side,
  amount: number,
): SideQuote {
  const cents = liquidity ? liquidity[side] : null;
  if (!liquidity || cents === null) return { kind: "unavailable" };
  if (amount > liquidity.limit) return { kind: "too-much" };
  return {
    kind: "ready",
    win: Math.floor((amount * 100) / cents),
    revision: `${side}-${cents}`,
  };
}

/** Preview Predictions. `now` is the clock its dates follow. */
export function usePreviewPredictions(now = new Date()): PredictionsSource {
  const { catalogue, hasFailed } = useBuilderCatalogue();
  const [collection, updateCollection] = useDemoCollection();
  const [store, setStore] = useAtom(predictionsAtom);
  const [quoteTarget, setQuoteTarget] = useState<{
    predictionId: string;
    amount: number;
  } | null>(null);
  const [quoteReady, setQuoteReady] = useState(false);
  const [cardQuery, setCardQuery] = useState<string | null>(null);
  const [priceTarget, setPriceTarget] = useState<string | null>(null);
  const [priceReady, setPriceReady] = useState(false);
  const [betPending, setBetPending] = useState(false);
  const [publishPending, setPublishPending] = useState(false);
  const [commandFailure, setCommandFailure] = useState<string | null>(null);
  const [completed, setCompleted] = useState<Completed | null>(null);
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
  useEffect(() => {
    if (!quoteTarget) return;
    setQuoteReady(false);
    const timer = window.setTimeout(() => setQuoteReady(true), QUOTE_MS);
    return () => window.clearTimeout(timer);
  }, [quoteTarget]);
  useEffect(() => {
    if (!priceTarget) return;
    setPriceReady(false);
    const timer = window.setTimeout(() => setPriceReady(true), QUOTE_MS);
    return () => window.clearTimeout(timer);
  }, [priceTarget]);

  const today = utcDay(now);
  const seeded = useMemo((): (Prediction & { liquidity?: Liquidity })[] => {
    if (!catalogue) return [];
    return SEEDS.flatMap((seed) => {
      const card = catalogue.find(seed.card);
      if (!card) return [];
      const deadline = addDays(today, seed.endsIn);
      const stored = store.bets[`preview-prediction-${seed.id}`];
      const bet = stored ?? seed.bet;
      const mine: MyBet = bet ? { kind: "bet", ...bet } : { kind: "none" };
      return [
        {
          id: `preview-prediction-${seed.id}`,
          card: printingOf(card, seed.finish ?? "Nonfoil"),
          targetUsd: seed.targetUsd,
          condition: seed.condition,
          deadline,
          closesAt: `${addDays(today, Math.min(seed.endsIn - 1, 5))}T12:00:00Z`,
          state: seed.state,
          accepting: seed.state === "open",
          outcome: seed.outcome ?? null,
          evidence: seed.evidence
            ? {
                date: addDays(deadline, -seed.evidence.daysBeforeEnd),
                usd: seed.evidence.usd,
              }
            : null,
          voidReason:
            seed.outcome === "void" ? "Price data was incomplete." : null,
          mine,
          authored: seed.authored ?? false,
          ...(seed.liquidity ? { liquidity: seed.liquidity } : {}),
        },
      ];
    });
  }, [catalogue, store.bets, today]);
  const nowIso = now.toISOString();
  const created = store.created.map((entry): Prediction => {
    const bet = store.bets[entry.id];
    return {
      id: entry.id,
      card: {
        printingId: entry.printingId,
        name: entry.name,
        setCode: entry.setCode,
        finish: entry.finish,
        faceUrl: entry.faceUrl,
      },
      targetUsd: entry.targetUsd,
      condition: entry.condition,
      deadline: entry.deadline,
      closesAt: entry.closesAt,
      state: nowIso < entry.closesAt ? "open" : "closed",
      accepting: nowIso < entry.closesAt,
      outcome: null,
      evidence: null,
      voidReason: null,
      mine: bet ? { kind: "bet", ...bet } : { kind: "none" },
      authored: true,
    };
  });
  const all = [...created, ...seeded];
  const liquidityOf = (id: string) =>
    seeded.find((prediction) => prediction.id === id)?.liquidity;

  const quoteFor = useCallback(
    (predictionId: string | null, amount: number) =>
      setQuoteTarget((current) =>
        predictionId === null
          ? null
          : current?.predictionId === predictionId && current.amount === amount
            ? current
            : { predictionId, amount },
      ),
    [],
  );
  const quote: Quote | null = quoteTarget
    ? {
        ...quoteTarget,
        yes: quoteReady
          ? previewSideQuote(
              liquidityOf(quoteTarget.predictionId),
              "yes",
              quoteTarget.amount,
            )
          : { kind: "checking" },
        no: quoteReady
          ? previewSideQuote(
              liquidityOf(quoteTarget.predictionId),
              "no",
              quoteTarget.amount,
            )
          : { kind: "checking" },
      }
    : null;

  const searchCards = useCallback(
    (query: string | null) => setCardQuery(query),
    [],
  );
  const cardItems = useMemo((): PrintingLabel[] => {
    if (!catalogue || cardQuery === null) return [];
    const needle = cardQuery.trim().toLowerCase();
    return catalogue.cards
      .filter((card) => card.name.toLowerCase().includes(needle))
      .flatMap((card) => [
        printingOf(card, "Nonfoil"),
        printingOf(card, "Foil"),
      ]);
  }, [catalogue, cardQuery]);
  const checkPrice = useCallback(
    (printingId: string | null) => setPriceTarget(printingId),
    [],
  );
  // Basic lands carry no tracked price in the preview.
  const price: PriceCheck | null = priceTarget
    ? !priceReady
      ? { printingId: priceTarget, state: "checking" }
      : priceTarget.includes("-mountain-")
        ? { printingId: priceTarget, state: "unavailable" }
        : { printingId: priceTarget, state: "ready" }
    : null;

  if (!catalogue)
    return hasFailed ? { status: "unavailable" } : { status: "loading" };
  const balance = coinsOf(collection);
  return {
    status: "ready",
    balance,
    explore: {
      items: all,
      state: "ready",
      isAppending: false,
      hasMore: false,
      loadMore: () => undefined,
    },
    mine: {
      bets: {
        items: all.filter((prediction) => prediction.mine.kind === "bet"),
        state: "ready",
      },
      created: {
        items: all.filter((prediction) => prediction.authored),
        state: "ready",
      },
    },
    failure: null,
    dismissFailure: () => undefined,
    quoteFor,
    quote,
    betBlock: null,
    placeBet: (predictionId, side, amount, revision) => {
      if (betPending) return;
      setBetPending(true);
      setCommandFailure(null);
      later(COMMAND_MS, () => {
        setBetPending(false);
        const target = all.find((prediction) => prediction.id === predictionId);
        const terms = previewSideQuote(liquidityOf(predictionId), side, amount);
        if (
          !target ||
          target.mine.kind === "bet" ||
          target.state !== "open" ||
          terms.kind !== "ready" ||
          terms.revision !== revision
        ) {
          setCommandFailure("Couldn’t place bet.");
          return;
        }
        if (coinsOf(collection) < amount) {
          setCommandFailure("Not enough coins.");
          return;
        }
        updateCollection((existing) => ({
          ...existing,
          coins: coinsOf(existing) - amount,
        }));
        setStore((existing) => ({
          ...existing,
          bets: {
            ...existing.bets,
            [predictionId]: { side, cost: amount, win: terms.win },
          },
        }));
        setCompleted({ kind: "bet" });
      });
    },
    betPending,
    searchCards,
    cards: {
      items: cardItems,
      state: cardQuery === null ? "idle" : "ready",
      isAppending: false,
      hasMore: false,
      loadMore: () => undefined,
    },
    checkPrice,
    price,
    publish: (draft) => {
      if (publishPending) return;
      setPublishPending(true);
      setCommandFailure(null);
      later(COMMAND_MS, () => {
        setPublishPending(false);
        const schedule = scheduleFrom(new Date());
        if (draft.deadline < schedule.earliest) {
          setCommandFailure("Couldn’t create prediction.");
          return;
        }
        const id = `preview-prediction-created-${Date.now().toString(36)}`;
        setStore((existing) => ({
          ...existing,
          created: [
            {
              id,
              printingId: draft.card.printingId,
              name: draft.card.name,
              setCode: draft.card.setCode,
              finish: draft.card.finish,
              faceUrl: draft.card.faceUrl,
              targetUsd: draft.targetUsd,
              condition: draft.condition,
              deadline: draft.deadline,
              closesAt: draft.schedule.closesAt,
            },
            ...existing.created,
          ],
        }));
        setCompleted({ kind: "created", predictionId: id });
      });
    },
    publishPending,
    commandFailure,
    dismissCommandFailure: () => setCommandFailure(null),
    completed,
    dismissCompleted: () => setCompleted(null),
  };
}
