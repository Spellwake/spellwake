import { useCallback, useMemo, useRef, useState } from "react";
import { useAtomValue } from "jotai";
import type { PlayerSnapshot } from "@spellwake/application-contracts";
import { useBuilderCatalogue } from "../data/builder-catalogue";
import {
  catalogueName,
  PREVIEW_FACES,
  PREVIEW_GAME,
  PREVIEW_SEATS,
  PREVIEW_VIEWER,
  previewAttackDecision,
  previewBlockDecision,
  previewCast,
  previewCastOffers,
  previewConcede,
  previewDamageDecision,
  previewDivisionDecision,
  previewResolveTop,
  previewResponses,
  previewSnapshot,
  type PreviewDivision,
} from "./preview-table";
import { printedModes, type PrintedModes } from "./printed-modes";
import { matchPreferences, readPreferences } from "./table-panels";
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

/** Apply a preview land drop while keeping the viewer's handCount invariant in sync. */
export function applyDemoPlayLand(
  prev: PlayerSnapshot,
  source: string,
): PlayerSnapshot {
  const nextObjs = prev.visibleObjects.map((o) =>
    o.handle === source ? { ...o, zone: "zone-battlefield" } : o,
  );
  let removedFromHand = false;
  const nextZones = prev.zones.map((z) => {
    if (z.handle === "zone-hand-you" && z.visibility === "visible") {
      const filtered = z.objects.filter((h) => h !== source);
      removedFromHand = filtered.length !== z.objects.length;
      return {
        ...z,
        objects: filtered,
      };
    }
    if (z.handle === "zone-battlefield" && z.visibility === "visible") {
      return { ...z, objects: [...z.objects, source] };
    }
    return z;
  });
  const nextPlayers = removedFromHand
    ? prev.players.map((p) =>
        p.seatId === VIEWER_SEAT
          ? {
              ...p,
              handCount: String(Math.max(0, Number(p.handCount) - 1)),
            }
          : p,
      )
    : prev.players;
  return {
    ...prev,
    players: nextPlayers,
    visibleObjects: nextObjs,
    zones: nextZones,
  };
}

/** Local interactive preview match for unconfigured builds. */
export function useDemoMatch(
  onLeave: () => void,
  /** Where the table starts; the development probe starts part-way through combat. */
  initial: () => PlayerSnapshot = previewSnapshot,
): MatchSource {
  const [snapshot, setSnapshot] = useState<PlayerSnapshot>(initial);
  const [commandError, setCommandError] = useState<string | null>(null);
  const [messages, setMessages] = useState<MatchChatEntry[]>(() =>
    (
      [
        [1, "Good luck, everyone."],
        [2, "Good luck!"],
        [3, "Nice opening."],
        [4, "Your turn, Alex."],
        [5, "Ready."],
      ] as const
    ).map(([seat, body], index) => ({
      id: `demo-chat-${index + 1}`,
      senderId: PREVIEW_SEATS[seat].seatId,
      senderName: PREVIEW_SEATS[seat].name,
      body,
      sentAt: new Date(Date.UTC(2026, 10, 22, 18, index)).toISOString(),
      isMine: false,
    })),
  );

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
      const own = PREVIEW_FACES[name];
      if (found) faces.set(name, found.faceUrl);
      else if (own)
        faces.set(name, `${import.meta.env.BASE_URL}match-preview/${own.file}`);
    }
    return faces;
  }, [catalogue]);

  const printedModesByName = useMemo(() => {
    const printed = new Map<string, PrintedModes>();
    for (const object of previewSnapshot().visibleObjects) {
      if (object.identity.kind !== "known") continue;
      const name = object.identity.characteristics.name;
      const regions = printedModes(
        catalogue?.find(catalogueName(name))?.printing ??
          PREVIEW_FACES[name]?.printing,
      );
      if (regions) printed.set(name, regions);
    }
    return printed;
  }, [catalogue]);

  // How each divided spell on the stack was shared out, by its stack item.
  const divisions = useRef(new Map<string, PreviewDivision>());
  const sendAction = useCallback(
    async (action: MatchActionCommand) => {
      setCommandError(null);
      setSnapshot((prev) => {
        if (action.kind === "pass" && prev.turn) {
          // With something waiting, a pass lets the newest item resolve.
          const top = prev.stack.at(-1);
          if (top)
            return previewResolveTop(prev, divisions.current.get(top.handle));
          const idx = PHASE_ORDER.findIndex(
            (p) => p.phase === prev.turn?.phase,
          );
          const next = PHASE_ORDER[(idx + 1) % PHASE_ORDER.length]!;
          const wrapped = idx === PHASE_ORDER.length - 1;
          const wasMine = prev.turn.activeSeatIds.includes(VIEWER_SEAT);
          // The preview alternates your turn with Alex's.
          const active = wrapped
            ? wasMine
              ? OPPONENT_SEAT
              : VIEWER_SEAT
            : (prev.turn.activeSeatIds[0] ?? VIEWER_SEAT);
          const moved: PlayerSnapshot = {
            ...prev,
            throughEventSequence: String(Number(prev.throughEventSequence) + 1),
            // A new turn untaps its player's permanents.
            visibleObjects: wrapped
              ? prev.visibleObjects.map((object) =>
                  object.controllerSeatId === active && object.tapped
                    ? { ...object, tapped: false }
                    : object,
                )
              : prev.visibleObjects,
            players: prev.players.map((player) =>
              player.manaPool.length > 0 ? { ...player, manaPool: [] } : player,
            ),
            turn: {
              ...prev.turn,
              number: wrapped
                ? String(Number(prev.turn.number) + 1)
                : prev.turn.number,
              activeSeatIds: [active],
              phase: next.phase,
              step: next.step,
            },
          };
          // On Alex's first main phase a burn spell is cast and answered.
          if (next.phase === "precombat-main" && active === OPPONENT_SEAT)
            return previewResponses(moved);
          if (next.phase !== "combat") return moved;
          if (active === VIEWER_SEAT) {
            const offered = previewAttackDecision(moved);
            return offered ? { ...moved, decisions: [offered] } : moved;
          }
          // Alex attacks you: the attackers tap, and you choose blocks.
          const offered = previewBlockDecision(moved, active);
          if (!offered || offered.domain.kind !== "blockers") return moved;
          const attacking = new Set(
            offered.domain.choices.flatMap((choice) => choice.attackers),
          );
          return {
            ...moved,
            turn: { ...moved.turn!, step: "declare-blockers" },
            visibleObjects: moved.visibleObjects.map((object) =>
              attacking.has(object.handle)
                ? { ...object, tapped: true }
                : object,
            ),
            decisions: [offered],
          };
        }
        if (action.kind === "choose") {
          const response = action.response;
          const settled = {
            ...prev,
            decisions: [],
            throughEventSequence: String(Number(prev.throughEventSequence) + 1),
          };
          // A divided spell's shares are kept for when it resolves.
          const divided = prev.stack.at(-1);
          if (response.kind === "distribution" && divided)
            divisions.current.set(divided.handle, response.allocations);
          if (response.kind !== "attackers") return settled;
          // Declared attackers tap, as the returned table shows.
          const attacking = new Set(
            response.assignments.map((entry) => entry.attacker),
          );
          const tapped: PlayerSnapshot = {
            ...settled,
            visibleObjects: prev.visibleObjects.map((object) =>
              attacking.has(object.handle)
                ? { ...object, tapped: true }
                : object,
            ),
          };
          // The first creature sent at a player is double-blocked where it can be.
          const first = response.assignments.find(
            (entry) => entry.defender.kind === "seat",
          );
          const offered =
            first?.defender.kind === "seat"
              ? previewDamageDecision(tapped, [
                  {
                    attacker: first.attacker,
                    defenderSeat: first.defender.seatId,
                  },
                ])
              : null;
          return offered
            ? {
                ...tapped,
                turn: { ...tapped.turn!, step: "combat-damage" },
                decisions: [offered],
              }
            : tapped;
        }
        if (action.kind === "cast-targeted")
          return previewCast(prev, action.source, [action.target]);
        if (action.kind === "cast-untargeted")
          return previewCast(prev, action.source, []);
        // A modal spell carries its chosen modes, each with its own target;
        // a spell with several instructions carries their targets in order.
        if (action.kind === "cast-with-resources") {
          const modes = action.modes ?? [];
          const cast = previewCast(
            prev,
            action.source,
            action.fixedTargets ??
              modes.flatMap((mode) => (mode.target ? [mode.target] : [])),
            modes.length > 0 ? modes.map((mode) => mode.mode) : undefined,
          );
          // A divided spell is cast first, then asked how its amount is shared.
          const asked = previewDivisionDecision(cast);
          return asked ? { ...cast, decisions: [asked] } : cast;
        }
        if (action.kind === "play-land") {
          return applyDemoPlayLand(prev, action.source);
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
        // Conceding keeps you at the table to watch.
        if (action.kind === "concede") return previewConcede(prev);
        return prev;
      });
    },
    [onLeave],
  );

  // The preview stands in for cast offers the game does not send yet. With
  // Auto-tap mana off it pauses for payment, as a manual game would.
  const { autoTap } = readPreferences(useAtomValue(matchPreferences));
  const castOffers = useMemo(
    () => previewCastOffers(snapshot, autoTap),
    [snapshot, autoTap],
  );

  return {
    status: "ready",
    matchId: "demo-match",
    gameId: DEMO_GAME_ID,
    snapshot,
    seatNames,
    artByPrintingId: new Map(),
    faceByName,
    printedModesByName,
    castOffers,
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
