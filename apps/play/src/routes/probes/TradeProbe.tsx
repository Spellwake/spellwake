import type { ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { CENTRED } from "../../account/portrait-crop";
import { useBuilderCatalogue } from "../../data/builder-catalogue";
import {
  COINS_ID,
  type OfferLine,
  type ReadyTrade,
  type TradeHolding,
  type TradeSource,
} from "../../trade/trade-sources";
import { TradePage } from "../trade/TradePage";

/**
 * Development only: Trade in states the preview cannot reach on its own, for
 * the stress checks and review captures. `?state=` names one.
 */
const noop = (): void => undefined;

export function TradeProbe(): ReactElement {
  const [params] = useSearchParams();
  const state = params.get("state") ?? "editing";
  const { catalogue } = useBuilderCatalogue();
  const art = (name: string) => catalogue?.find(name)?.artUrl ?? null;
  const card = (name: string, detail: string, quantity: number): OfferLine => ({
    id: `card:${name}:nonfoil`,
    kind: "card",
    name,
    detail,
    artUrl: art(name),
    setName: null,
    quantity,
  });
  const pack = (setName: string, quantity: number): OfferLine => ({
    id: `preview-pack-${setName.slice(0, 3).toLowerCase()}`,
    kind: "pack",
    name: `${setName} Play Booster`,
    detail: null,
    artUrl: null,
    setName,
    quantity,
  });
  const coins = (quantity: number): OfferLine => ({
    id: COINS_ID,
    kind: "coin",
    name: "Coins",
    detail: null,
    artUrl: null,
    setName: null,
    quantity,
  });
  const offer = [
    card("Lightning Bolt", "MSC · Instant", 2),
    card("Goblin Guide", "2XM · Creature", 1),
    coins(250),
  ];
  const dense = [
    card("Lightning Bolt", "MSC · Instant", 4),
    card("Goblin Guide", "2XM · Creature", 1),
    card("Shock", "M21 · Instant", 4),
    card("Goblin Matron", "MH2 · Creature", 2),
    card("Mountain", "M11 · Basic Land", 20),
    coins(250),
  ];
  const holdings: TradeHolding[] = [
    ...dense.map((line) => ({ ...line, owned: line.quantity + 2 })),
    { ...coins(0), owned: 1000 },
  ].filter((holding) => holding.kind !== "coin" || holding.owned > 0);
  const theirs = [
    card("Krenko, Mob Boss", "FDN · Legendary Creature", 1),
    pack("Foundations", 2),
  ];
  const ready = (overrides: Partial<ReadyTrade>): TradeSource => ({
    status: "ready",
    partner: {
      id: "mira",
      name: "Mira",
      portrait: art("Goblin Matron")
        ? { artUrl: art("Goblin Matron") ?? "", crop: CENTRED }
        : null,
    },
    own: { status: "editing", lines: offer },
    other: { status: "editing", lines: theirs },
    holdings,
    holdingsState: "ready",
    stage: { kind: "open" },
    pending: null,
    notice: null,
    dismissNotice: noop,
    setQuantity: noop,
    add: noop,
    ready: noop,
    editItems: noop,
    trade: noop,
    cancel: noop,
    checkStatus: noop,
    ...overrides,
  });
  const sources: Record<string, TradeSource> = {
    loading: { status: "loading" },
    unavailable: { status: "unavailable" },
    editing: ready({}),
    empty: ready({ own: { status: "editing", lines: [] } }),
    dense: ready({ own: { status: "editing", lines: dense } }),
    reserving: ready({ pending: "ready" }),
    rejected: ready({ notice: "Couldn’t update trade. No changes were made." }),
    short: ready({
      own: {
        status: "editing",
        lines: [card("Lightning Bolt", "MSC · Instant", 3), coins(1500)],
      },
      holdings: [
        { ...card("Lightning Bolt", "MSC · Instant", 0), owned: 2 },
        { ...coins(0), owned: 1000 },
      ],
    }),
    waiting: ready({ own: { status: "ready", lines: offer } }),
    "both-ready": ready({
      own: { status: "ready", lines: offer },
      other: { status: "ready", lines: theirs },
    }),
    confirmed: ready({
      own: { status: "confirmed", lines: offer },
      other: { status: "ready", lines: theirs },
    }),
    unconfirmed: ready({
      own: { status: "confirmed", lines: offer },
      other: { status: "ready", lines: theirs },
      stage: { kind: "unconfirmed", isChecking: false },
    }),
    checking: ready({
      own: { status: "confirmed", lines: offer },
      other: { status: "ready", lines: theirs },
      stage: { kind: "unconfirmed", isChecking: true },
    }),
    complete: ready({
      stage: { kind: "complete", gave: offer, received: theirs },
    }),
    canceled: ready({ stage: { kind: "closed", reason: "canceled" } }),
  };
  return (
    <TradePage
      source={sources[state] ?? ready({})}
      onBack={noop}
      onFinish={noop}
    />
  );
}
