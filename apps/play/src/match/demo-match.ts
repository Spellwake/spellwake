import { useCallback, useMemo, useState } from "react";
import type { PlayerSnapshot } from "@spellwake/application-contracts";
import { useBuilderCatalogue } from "../data/builder-catalogue";
import {
  catalogueName,
  PREVIEW_GAME,
  PREVIEW_SEATS,
  PREVIEW_VIEWER,
  previewSnapshot,
} from "./preview-table";
import type {
  MatchActionCommand,
  MatchChatEntry,
  MatchSource,
} from "./match-sources";

const VIEWER_SEAT = PREVIEW_VIEWER;
const OPPONENT_SEAT = PREVIEW_SEATS[1].seatId;
const DEMO_GAME_ID = PREVIEW_GAME;

const PHASE_ORDER = [
  { phase: "beginning", step: "upkeep" },
  { phase: "precombat-main", step: "precombat-main" },
  { phase: "combat", step: "declare-attackers" },
  { phase: "postcombat-main", step: "postcombat-main" },
  { phase: "ending", step: "end" },
] as const;

/** Local interactive preview match for unconfigured builds. */
export function useDemoMatch(onLeave: () => void): MatchSource {
  const [snapshot, setSnapshot] = useState<PlayerSnapshot>(previewSnapshot);
  const [commandError, setCommandError] = useState<string | null>(null);
  const [messages, setMessages] = useState<MatchChatEntry[]>([
    {
      id: "demo-chat-1",
      senderId: OPPONENT_SEAT,
      senderName: "Alex",
      body: "Good luck, have fun!",
      sentAt: new Date().toISOString(),
      isMine: false,
    },
  ]);

  const seatNames = useMemo(
    () =>
      new Map<string, string>(
        PREVIEW_SEATS.map((seat) => [seat.seatId, seat.name]),
      ),
    [],
  );
  // The preview's cards have no printing, so their pictures are found by name.
  const { catalogue } = useBuilderCatalogue();
  const faceByName = useMemo(() => {
    const faces = new Map<string, string>();
    for (const object of previewSnapshot().visibleObjects) {
      if (object.identity.kind !== "known") continue;
      const name = object.identity.characteristics.name;
      const found = catalogue?.find(catalogueName(name));
      if (found) faces.set(name, found.faceUrl);
    }
    return faces;
  }, [catalogue]);

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
    faceByName,
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
