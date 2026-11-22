import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { z } from "zod";
import {
  useBuilderCatalogue,
  type BuilderCard,
} from "../data/builder-catalogue";
import {
  coinsOf,
  useDemoCollection,
  type DemoCollection,
} from "../collection/demo-collection";
import { useDemoPerson } from "../messaging/demo-messaging";
import { useDemoListingHolds } from "../market/demo-market-store";
import {
  canMarkReady,
  capQuantity,
  COINS_ID,
  inTrayOrder,
  withQuantity,
  type OfferLine,
  type TradeAsset,
  type TradeHolding,
  type TradePending,
  type TradeSource,
} from "./trade-sources";

/**
 * The local preview of a trade room. The other person is simulated: they
 * become Ready a moment after you do and confirm a moment after you confirm.
 * Your offer, readiness and the packs it holds are kept in this browser, so
 * leaving and coming back resumes the room. A completed trade moves the
 * cards, packs and coins within the preview collection. Nothing reaches an
 * account or another person.
 */

/** How long the preview takes to answer a command, and the other person to follow you. */
const COMMAND_MS = 650;
const PARTNER_MS = 1600;
const REJECTED = "Couldn’t update trade. No changes were made.";

const lineSchema = z.strictObject({
  id: z.string().min(1),
  kind: z.enum(["card", "pack", "coin"]),
  name: z.string().min(1),
  detail: z.string().nullable(),
  artUrl: z.string().nullable(),
  setName: z.string().nullable(),
  quantity: z.number().int().positive(),
});
const readinessSchema = z.enum(["editing", "ready", "confirmed"]);
const roomSchema = z.strictObject({
  own: z.array(lineSchema),
  ownStatus: readinessSchema,
  otherStatus: readinessSchema,
  /** Packs this room holds in the preview collection while you are Ready. */
  heldPacks: z.record(z.string(), z.number().int().nonnegative()),
  stage: z.discriminatedUnion("kind", [
    z.strictObject({ kind: z.literal("open") }),
    z.strictObject({
      kind: z.literal("complete"),
      gave: z.array(lineSchema),
      received: z.array(lineSchema),
    }),
    z.strictObject({
      kind: z.literal("closed"),
      reason: z.enum(["canceled", "unavailable"]),
    }),
  ]),
});
type DemoRoom = z.infer<typeof roomSchema>;
const roomsSchema = z.record(z.string(), roomSchema);
type DemoRooms = z.infer<typeof roomsSchema>;

const NEW_ROOM: DemoRoom = {
  own: [],
  ownStatus: "editing",
  otherStatus: "editing",
  heldPacks: {},
  stage: { kind: "open" },
};

const storage = {
  getItem: (key: string, initial: DemoRooms): DemoRooms => {
    try {
      const saved = window.localStorage.getItem(key);
      if (saved === null) return initial;
      const parsed = roomsSchema.safeParse(JSON.parse(saved));
      return parsed.success ? parsed.data : initial;
    } catch {
      return initial;
    }
  },
  setItem: (key: string, value: DemoRooms): void => {
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
export const DEMO_TRADE_KEY = "spellwake:trade:demo:v1";
const roomsAtom = atomWithStorage<DemoRooms>(DEMO_TRADE_KEY, {}, storage, {
  getOnInit: true,
});

/** "M11 · Instant", from the printing's set and the type line before its dash. */
const cardDetail = (card: BuilderCard, foil: boolean): string =>
  [
    card.printing?.set ?? null,
    card.type.split(" — ")[0] ?? card.type,
    foil ? "Foil" : null,
  ]
    .filter((part): part is string => !!part)
    .join(" · ");
const cardId = (name: string, foil: boolean): string =>
  `card:${name}:${foil ? "foil" : "nonfoil"}`;
const cardAsset = (card: BuilderCard, foil: boolean): TradeAsset => ({
  id: cardId(card.name, foil),
  kind: "card",
  name: card.name,
  detail: cardDetail(card, foil),
  artUrl: card.artUrl,
  setName: null,
});
const COIN_ASSET: TradeAsset = {
  id: COINS_ID,
  kind: "coin",
  name: "Coins",
  detail: null,
  artUrl: null,
  setName: null,
};

/** What the simulated person offers: cards from the preview catalogue and two packs. */
const PARTNER_OFFER = [
  { card: "Krenko, Mob Boss", quantity: 1 },
  { pack: "Foundations", quantity: 2 },
] as const;

/** Your holdings in the preview collection, less what other trades hold. */
function holdingsOf(
  collection: DemoCollection,
  find: (name: string) => BuilderCard | undefined,
  room: DemoRoom,
  listed: Readonly<Record<string, number>>,
): TradeHolding[] {
  const cards = Object.entries(collection.cards).flatMap(([name, held]) => {
    const card = find(name);
    if (!card) return [];
    return [
      held.nonfoil > 0
        ? {
            ...cardAsset(card, false),
            owned: held.nonfoil - (listed[cardId(name, false)] ?? 0),
          }
        : null,
      held.foil > 0
        ? {
            ...cardAsset(card, true),
            owned: held.foil - (listed[cardId(name, true)] ?? 0),
          }
        : null,
    ].filter((entry): entry is TradeHolding => entry !== null);
  });
  const packs = collection.packs.map((pack): TradeHolding => ({
    id: pack.id,
    kind: "pack",
    name: pack.product,
    detail: null,
    artUrl: null,
    setName: pack.setName,
    // Packs this room holds stay offerable here; others' holds do not.
    owned:
      pack.owned -
      (pack.held - (room.heldPacks[pack.id] ?? 0)) -
      (listed[pack.id] ?? 0),
  }));
  return [
    ...cards.sort((a, b) => a.name.localeCompare(b.name)),
    ...packs,
    { ...COIN_ASSET, owned: coinsOf(collection) },
  ].filter((holding) => holding.owned > 0);
}

/** Holds or releases this room's packs in the preview collection. */
function holdPacks(
  collection: DemoCollection,
  change: Record<string, number>,
): DemoCollection {
  return {
    ...collection,
    packs: collection.packs.map((pack) => ({
      ...pack,
      held: Math.max(
        0,
        Math.min(pack.owned, pack.held + (change[pack.id] ?? 0)),
      ),
    })),
  };
}

/** Moves what each side gave, releasing the room's holds. */
function settle(
  collection: DemoCollection,
  room: DemoRoom,
  received: readonly OfferLine[],
): DemoCollection {
  const cards = { ...collection.cards };
  const packs = collection.packs.map((pack) => ({ ...pack }));
  let coins = coinsOf(collection);
  const move = (line: OfferLine, sign: 1 | -1) => {
    const amount = sign * line.quantity;
    if (line.kind === "coin") coins = Math.max(0, coins + amount);
    else if (line.kind === "card") {
      const foil = line.id.endsWith(":foil");
      const held = cards[line.name] ?? { nonfoil: 0, foil: 0 };
      cards[line.name] = foil
        ? { ...held, foil: Math.max(0, held.foil + amount) }
        : { ...held, nonfoil: Math.max(0, held.nonfoil + amount) };
    } else {
      const pack = packs.find(
        (entry) => entry.id === line.id || entry.product === line.name,
      );
      if (pack) {
        pack.owned = Math.max(0, pack.owned + amount);
        if (sign < 0)
          pack.held = Math.max(0, pack.held - (room.heldPacks[pack.id] ?? 0));
      } else if (sign > 0 && line.setName)
        packs.push({
          id: line.id,
          product: line.name,
          setName: line.setName,
          setCode: line.setName.slice(0, 3).toUpperCase(),
          owned: line.quantity,
          held: 0,
        });
    }
  };
  for (const line of room.own) move(line, -1);
  for (const line of received) move(line, 1);
  return {
    ...collection,
    cards: Object.fromEntries(
      Object.entries(cards).filter(([, held]) => held.nonfoil + held.foil > 0),
    ),
    packs: packs
      .map((pack) => ({ ...pack, held: Math.min(pack.held, pack.owned) }))
      .filter((pack) => pack.owned > 0),
    coins,
  };
}

/** The preview room with one preview person, by their Messages id. */
export function useDemoTrade(partnerId: string): TradeSource {
  const person = useDemoPerson(partnerId);
  const { catalogue, hasFailed } = useBuilderCatalogue();
  const [collection, updateCollection] = useDemoCollection();
  const [rooms, setRooms] = useAtom(roomsAtom);
  // Your preview market listings hold their cards and packs out of trades.
  const listed = useDemoListingHolds();
  const room = rooms[partnerId] ?? NEW_ROOM;
  const [pending, setPending] = useState<TradePending>(null);
  const [notice, setNotice] = useState<string | null>(null);
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
  const change = useCallback(
    (update: (current: DemoRoom) => DemoRoom) =>
      setRooms((current) => ({
        ...current,
        [partnerId]: update(current[partnerId] ?? NEW_ROOM),
      })),
    [setRooms, partnerId],
  );

  const find = useCallback(
    (name: string) => catalogue?.find(name),
    [catalogue],
  );
  const holdings = useMemo(
    () => (catalogue ? holdingsOf(collection, find, room, listed) : []),
    [catalogue, collection, find, room, listed],
  );
  const other = useMemo((): OfferLine[] => {
    if (!catalogue) return [];
    return PARTNER_OFFER.flatMap((entry): OfferLine[] => {
      if ("card" in entry) {
        const card = find(entry.card);
        return card
          ? [{ ...cardAsset(card, false), quantity: entry.quantity }]
          : [];
      }
      return [
        {
          id: `preview-pack-${entry.pack.slice(0, 3).toLowerCase()}`,
          kind: "pack",
          name: `${entry.pack} Play Booster`,
          detail: null,
          artUrl: null,
          setName: entry.pack,
          quantity: entry.quantity,
        },
      ];
    });
  }, [catalogue, find]);

  // The other person follows you: Ready after you are, then confirming after you do.
  const { ownStatus, otherStatus, stage } = room;
  useEffect(() => {
    if (stage.kind !== "open") return;
    if (ownStatus === "ready" && otherStatus === "editing") {
      const timer = window.setTimeout(
        () =>
          change((current) =>
            current.ownStatus === "ready" && current.otherStatus === "editing"
              ? { ...current, otherStatus: "ready" }
              : current,
          ),
        PARTNER_MS,
      );
      return () => window.clearTimeout(timer);
    }
    if (ownStatus === "confirmed" && otherStatus === "ready") {
      const timer = window.setTimeout(() => {
        updateCollection((current) => settle(current, room, other));
        change((current) => ({
          ...current,
          otherStatus: "confirmed",
          heldPacks: {},
          stage: { kind: "complete", gave: current.own, received: other },
        }));
      }, PARTNER_MS);
      return () => window.clearTimeout(timer);
    }
    return undefined;
  }, [
    ownStatus,
    otherStatus,
    stage.kind,
    change,
    updateCollection,
    room,
    other,
  ]);

  // A person who blocks you or leaves closes the room.
  const isGone = person !== null && !person.canContact;
  useEffect(() => {
    if (isGone && stage.kind === "open")
      change((current) => ({
        ...current,
        heldPacks: {},
        stage: { kind: "closed", reason: "unavailable" },
      }));
  }, [isGone, stage.kind, change]);

  if (!person) return { status: "unavailable" };
  const held = (id: string) =>
    holdings.find((holding) => holding.id === id)?.owned ?? 0;
  const assetOf = (id: string): TradeAsset | undefined =>
    holdings.find((holding) => holding.id === id) ??
    room.own.find((line) => line.id === id);
  const isEditing = room.ownStatus === "editing" && room.stage.kind === "open";

  const setQuantity = (id: string, quantity: number) => {
    const asset = assetOf(id);
    if (!asset || !isEditing || pending) return;
    change((current) => ({
      ...current,
      own: inTrayOrder(
        withQuantity(current.own, asset, capQuantity(quantity, held(id))),
      ),
    }));
  };
  return {
    status: "ready",
    partner: { id: partnerId, name: person.name, portrait: person.portrait },
    own: { status: room.ownStatus, lines: inTrayOrder(room.own) },
    other: { status: room.otherStatus, lines: other },
    holdings,
    holdingsState: hasFailed ? "failed" : catalogue ? "ready" : "loading",
    stage: room.stage,
    pending,
    notice,
    dismissNotice: () => setNotice(null),
    setQuantity,
    add: (id) => {
      const current = room.own.find((line) => line.id === id)?.quantity ?? 0;
      setQuantity(id, current + 1);
    },
    ready: () => {
      if (!isEditing || pending) return;
      setPending("ready");
      later(COMMAND_MS, () => {
        setPending(null);
        if (!canMarkReady(room.own, other, holdings)) {
          setNotice(REJECTED);
          return;
        }
        // Ready reserves this side: its packs are held in the preview collection.
        const packs = Object.fromEntries(
          room.own
            .filter((line) => line.kind === "pack")
            .map((line) => [line.id, line.quantity]),
        );
        updateCollection((current) => holdPacks(current, packs));
        change((current) => ({
          ...current,
          ownStatus: "ready",
          heldPacks: packs,
        }));
      });
    },
    editItems: () => {
      if (room.ownStatus === "editing" || pending) return;
      setPending("edit");
      later(COMMAND_MS / 2, () => {
        setPending(null);
        const release = Object.fromEntries(
          Object.entries(room.heldPacks).map(([id, count]) => [id, -count]),
        );
        updateCollection((current) => holdPacks(current, release));
        change((current) => ({
          ...current,
          ownStatus: "editing",
          otherStatus: "editing",
          heldPacks: {},
        }));
      });
    },
    trade: () => {
      if (
        room.ownStatus !== "ready" ||
        room.otherStatus === "editing" ||
        pending
      )
        return;
      setPending("trade");
      later(COMMAND_MS, () => {
        setPending(null);
        change((current) => ({ ...current, ownStatus: "confirmed" }));
      });
    },
    cancel: () => {
      if (room.stage.kind !== "open" || pending) return;
      setPending("cancel");
      later(COMMAND_MS, () => {
        setPending(null);
        const release = Object.fromEntries(
          Object.entries(room.heldPacks).map(([id, count]) => [id, -count]),
        );
        updateCollection((current) => holdPacks(current, release));
        change((current) => ({
          ...current,
          ownStatus: "editing",
          otherStatus: "editing",
          heldPacks: {},
          stage: { kind: "closed", reason: "canceled" },
        }));
      });
    },
    // The preview always answers, so a Trade is never left unconfirmed.
    checkStatus: () => undefined,
  };
}

/** Forgets a finished preview room, so the next Trade with that person starts afresh. */
export function useForgetDemoTrade(): (partnerId: string) => void {
  const [, setRooms] = useAtom(roomsAtom);
  return useCallback(
    (partnerId: string) =>
      setRooms((current) => {
        const stage = current[partnerId]?.stage.kind;
        if (stage !== "complete" && stage !== "closed") return current;
        return Object.fromEntries(
          Object.entries(current).filter(([id]) => id !== partnerId),
        );
      }),
    [setRooms],
  );
}
