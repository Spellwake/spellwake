import { useCallback, useMemo, useState } from "react";
import type { PlayerSnapshot } from "@spellwake/application-contracts";
import type {
  MatchActionCommand,
  MatchChatEntry,
  MatchSource,
} from "./match-sources";

const VIEWER_SEAT = "00000000-0000-4000-8000-000000000101";
const OPPONENT_SEAT = "00000000-0000-4000-8000-000000000102";
const DEMO_GAME_ID = "00000000-0000-4000-8000-000000000201";
const DEMO_CONTEXT_ID = "00000000-0000-4000-8000-000000000202";

const INITIAL_SNAPSHOT: PlayerSnapshot = {
  basis: {
    gameId: DEMO_GAME_ID,
    contextId: DEMO_CONTEXT_ID,
    epoch: "1",
    revision: "12",
  },
  viewerSeatId: VIEWER_SEAT,
  players: [
    {
      seatId: VIEWER_SEAT,
      life: "20",
      poison: "0",
      counters: [],
      manaPool: [{ color: "red", amount: "1" }],
      handCount: "3",
      libraryCount: "48",
      eliminated: false,
    },
    {
      seatId: OPPONENT_SEAT,
      life: "17",
      poison: "0",
      counters: [],
      manaPool: [],
      handCount: "4",
      libraryCount: "49",
      eliminated: false,
    },
  ],
  turn: {
    number: "3",
    activeSeatIds: [VIEWER_SEAT],
    prioritySeatIds: [VIEWER_SEAT],
    phase: "precombat-main",
    step: "precombat-main",
  },
  visibleObjects: [
    {
      handle: "obj-mountain-1",
      zone: "zone-battlefield",
      ownerSeatId: VIEWER_SEAT,
      controllerSeatId: VIEWER_SEAT,
      identity: {
        kind: "known",
        definitionId: null,
        printingId: null,
        characteristics: {
          name: "Mountain",
          typeLine: "Basic Land — Mountain",
          rulesText: "({T}: Add {R}.)",
          manaCost: null,
          printedPower: null,
          printedToughness: null,
          printedLoyalty: null,
        },
        currentNames: ["Mountain"],
      },
      publicProperties: {
        cardTypes: ["land"],
        supertypes: ["basic"],
        landTypes: ["Mountain"],
        creatureTypes: [],
        colors: [],
        keywords: [],
        power: null,
        toughness: null,
      },
      tapped: false,
      counters: [],
      abilities: [{ handle: "ab-m1", kind: "mana", label: "{T}: Add {R}." }],
      damageMarked: "0",
      attachedTo: null,
    },
    {
      handle: "obj-goblin-1",
      zone: "zone-battlefield",
      ownerSeatId: VIEWER_SEAT,
      controllerSeatId: VIEWER_SEAT,
      identity: {
        kind: "known",
        definitionId: null,
        printingId: null,
        characteristics: {
          name: "Goblin Guide",
          typeLine: "Creature — Goblin Scout",
          rulesText:
            "Haste\nWhenever Goblin Guide attacks, defending player reveals the top card of their library.",
          manaCost: "{R}",
          printedPower: "2",
          printedToughness: "2",
          printedLoyalty: null,
        },
        currentNames: ["Goblin Guide"],
      },
      publicProperties: {
        cardTypes: ["creature"],
        supertypes: [],
        landTypes: [],
        creatureTypes: ["Goblin", "Scout"],
        colors: ["red"],
        keywords: ["haste"],
        power: "2",
        toughness: "2",
      },
      tapped: false,
      counters: [],
      abilities: [],
      damageMarked: "0",
      attachedTo: null,
    },
    {
      handle: "obj-island-1",
      zone: "zone-battlefield",
      ownerSeatId: OPPONENT_SEAT,
      controllerSeatId: OPPONENT_SEAT,
      identity: {
        kind: "known",
        definitionId: null,
        printingId: null,
        characteristics: {
          name: "Island",
          typeLine: "Basic Land — Island",
          rulesText: "({T}: Add {U}.)",
          manaCost: null,
          printedPower: null,
          printedToughness: null,
          printedLoyalty: null,
        },
        currentNames: ["Island"],
      },
      publicProperties: {
        cardTypes: ["land"],
        supertypes: ["basic"],
        landTypes: ["Island"],
        creatureTypes: [],
        colors: [],
        keywords: [],
        power: null,
        toughness: null,
      },
      tapped: false,
      counters: [],
      abilities: [{ handle: "ab-i1", kind: "mana", label: "{T}: Add {U}." }],
      damageMarked: "0",
      attachedTo: null,
    },
    {
      handle: "obj-bolt-hand",
      zone: "zone-hand-you",
      ownerSeatId: VIEWER_SEAT,
      controllerSeatId: VIEWER_SEAT,
      identity: {
        kind: "known",
        definitionId: null,
        printingId: null,
        characteristics: {
          name: "Lightning Bolt",
          typeLine: "Instant",
          rulesText: "Lightning Bolt deals 3 damage to any target.",
          manaCost: "{R}",
          printedPower: null,
          printedToughness: null,
          printedLoyalty: null,
        },
        currentNames: ["Lightning Bolt"],
      },
      publicProperties: {
        cardTypes: ["instant"],
        supertypes: [],
        landTypes: [],
        creatureTypes: [],
        colors: ["red"],
        keywords: [],
        power: null,
        toughness: null,
      },
      tapped: false,
      counters: [],
      abilities: [],
      damageMarked: "0",
      attachedTo: null,
    },
    {
      handle: "obj-mountain-hand",
      zone: "zone-hand-you",
      ownerSeatId: VIEWER_SEAT,
      controllerSeatId: VIEWER_SEAT,
      identity: {
        kind: "known",
        definitionId: null,
        printingId: null,
        characteristics: {
          name: "Mountain",
          typeLine: "Basic Land — Mountain",
          rulesText: "({T}: Add {R}.)",
          manaCost: null,
          printedPower: null,
          printedToughness: null,
          printedLoyalty: null,
        },
        currentNames: ["Mountain"],
      },
      publicProperties: {
        cardTypes: ["land"],
        supertypes: ["basic"],
        landTypes: ["Mountain"],
        creatureTypes: [],
        colors: [],
        keywords: [],
        power: null,
        toughness: null,
      },
      tapped: false,
      counters: [],
      abilities: [],
      damageMarked: "0",
      attachedTo: null,
    },
    {
      handle: "obj-monastery-hand",
      zone: "zone-hand-you",
      ownerSeatId: VIEWER_SEAT,
      controllerSeatId: VIEWER_SEAT,
      identity: {
        kind: "known",
        definitionId: null,
        printingId: null,
        characteristics: {
          name: "Monastery Swiftspear",
          typeLine: "Creature — Human Monk",
          rulesText: "Haste, prowess",
          manaCost: "{R}",
          printedPower: "1",
          printedToughness: "2",
          printedLoyalty: null,
        },
        currentNames: ["Monastery Swiftspear"],
      },
      publicProperties: {
        cardTypes: ["creature"],
        supertypes: [],
        landTypes: [],
        creatureTypes: ["Human", "Monk"],
        colors: ["red"],
        keywords: ["haste", "prowess"],
        power: "1",
        toughness: "2",
      },
      tapped: false,
      counters: [],
      abilities: [],
      damageMarked: "0",
      attachedTo: null,
    },
  ],
  zones: [
    {
      handle: "zone-battlefield",
      zoneType: "battlefield",
      ownerSeatId: null,
      visibility: "visible",
      objects: ["obj-mountain-1", "obj-goblin-1", "obj-island-1"],
      orderKnown: false,
    },
    {
      handle: "zone-stack",
      zoneType: "stack",
      ownerSeatId: null,
      visibility: "visible",
      objects: [],
      orderKnown: true,
    },
    {
      handle: "zone-exile",
      zoneType: "exile",
      ownerSeatId: null,
      visibility: "visible",
      objects: [],
      orderKnown: false,
    },
    {
      handle: "zone-command",
      zoneType: "command",
      ownerSeatId: null,
      visibility: "visible",
      objects: [],
      orderKnown: false,
    },
    {
      handle: "zone-hand-you",
      zoneType: "hand",
      ownerSeatId: VIEWER_SEAT,
      visibility: "visible",
      objects: ["obj-bolt-hand", "obj-mountain-hand", "obj-monastery-hand"],
      orderKnown: false,
    },
    {
      handle: "zone-library-you",
      zoneType: "library",
      ownerSeatId: VIEWER_SEAT,
      visibility: "hidden",
      hiddenCount: "48",
    },
    {
      handle: "zone-graveyard-you",
      zoneType: "graveyard",
      ownerSeatId: VIEWER_SEAT,
      visibility: "visible",
      objects: [],
      orderKnown: true,
    },
    {
      handle: "zone-hand-opp",
      zoneType: "hand",
      ownerSeatId: OPPONENT_SEAT,
      visibility: "hidden",
      hiddenCount: "4",
    },
    {
      handle: "zone-library-opp",
      zoneType: "library",
      ownerSeatId: OPPONENT_SEAT,
      visibility: "hidden",
      hiddenCount: "49",
    },
    {
      handle: "zone-graveyard-opp",
      zoneType: "graveyard",
      ownerSeatId: OPPONENT_SEAT,
      visibility: "visible",
      objects: [],
      orderKnown: true,
    },
  ],
  stack: [],
  decisions: [],
  throughEventSequence: "12",
};

const PHASE_ORDER = [
  { phase: "beginning", step: "upkeep" },
  { phase: "precombat-main", step: "precombat-main" },
  { phase: "combat", step: "declare-attackers" },
  { phase: "postcombat-main", step: "postcombat-main" },
  { phase: "ending", step: "end" },
] as const;

/** Local interactive preview match for unconfigured builds. */
export function useDemoMatch(onLeave: () => void): MatchSource {
  const [snapshot, setSnapshot] = useState<PlayerSnapshot>(INITIAL_SNAPSHOT);
  const [commandError, setCommandError] = useState<string | null>(null);
  const [messages, setMessages] = useState<MatchChatEntry[]>([
    {
      id: "demo-chat-1",
      senderId: OPPONENT_SEAT,
      senderName: "Mira",
      body: "Good luck, have fun!",
      sentAt: new Date().toISOString(),
      isMine: false,
    },
  ]);

  const seatNames = useMemo(
    () =>
      new Map<string, string>([
        [VIEWER_SEAT, "You"],
        [OPPONENT_SEAT, "Mira"],
      ]),
    [],
  );

  const sendAction = useCallback(
    async (action: MatchActionCommand) => {
      setCommandError(null);
      setSnapshot((prev) => {
        if (action.kind === "pass" && prev.turn) {
          const idx = PHASE_ORDER.findIndex(
            (p) => p.phase === prev.turn?.phase,
          );
          const next = PHASE_ORDER[(idx + 1) % PHASE_ORDER.length]!;
          const wrapped = idx === PHASE_ORDER.length - 1;
          return {
            ...prev,
            turn: {
              ...prev.turn,
              number: wrapped
                ? String(Number(prev.turn.number) + 1)
                : prev.turn.number,
              phase: next.phase,
              step: next.step,
            },
          };
        }
        if (action.kind === "play-land") {
          const nextObjs = prev.visibleObjects.map((o) =>
            o.handle === action.source ? { ...o, zone: "zone-battlefield" } : o,
          );
          const nextZones = prev.zones.map((z) => {
            if (z.handle === "zone-hand-you" && z.visibility === "visible") {
              return {
                ...z,
                objects: z.objects.filter((h) => h !== action.source),
              };
            }
            if (z.handle === "zone-battlefield" && z.visibility === "visible") {
              return { ...z, objects: [...z.objects, action.source] };
            }
            return z;
          });
          return { ...prev, visibleObjects: nextObjs, zones: nextZones };
        }
        if (action.kind === "activate-mana") {
          return {
            ...prev,
            visibleObjects: prev.visibleObjects.map((o) =>
              o.handle === action.source ? { ...o, tapped: true } : o,
            ),
            players: prev.players.map((p) =>
              p.seatId === VIEWER_SEAT
                ? {
                    ...p,
                    manaPool: [
                      {
                        color: "red",
                        amount: String(
                          Number(
                            p.manaPool.find((m) => m.color === "red")?.amount ??
                              "0",
                          ) + 1,
                        ),
                      },
                    ],
                  }
                : p,
            ),
          };
        }
        if (action.kind === "concede") {
          onLeave();
        }
        return prev;
      });
    },
    [onLeave],
  );

  return {
    status: "ready",
    matchId: "demo-match",
    gameId: DEMO_GAME_ID,
    snapshot,
    seatNames,
    artByPrintingId: new Map(),
    connectionState: "live",
    commandPending: false,
    commandError,
    dismissCommandError: () => setCommandError(null),
    sendAction,
    chooseOpening: null,
    chat: {
      messages,
      send: async (body) => {
        const trimmed = body.trim();
        if (!trimmed) return false;
        setMessages((curr) => [
          ...curr,
          {
            id: crypto.randomUUID(),
            senderId: VIEWER_SEAT,
            senderName: "You",
            body: trimmed,
            sentAt: new Date().toISOString(),
            isMine: true,
          },
        ]);
        return true;
      },
    },
    onLeave,
  };
}
