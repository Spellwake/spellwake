import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CENTRED } from "../account/portrait-crop";
import type { Portrait } from "../account/account-sources";
import { useDemoIdentity } from "../account/demo-account";
import { useBuilderCatalogue } from "../data/builder-catalogue";
import { DEMO_FORMATS } from "../collection/data/demo-deck-presets";
import {
  failed,
  succeeded,
  type Outcome,
  type PlayMode,
} from "../decks/deck-sources";
import { useDemoDeckShelf } from "../decks/demo-sources";
import {
  canStart,
  RANDOM_DECK,
  type AiDifficulty,
  type Departure,
  type InviteCandidate,
  type LobbyDeck,
  type LobbyRules,
  type LobbySeat,
  type LobbySource,
  type Phase,
} from "./lobby-sources";

/** The countdown every start runs, as approved. */
export const COUNTDOWN_MS = 10_000;
const STARTING_MS = 1400;
/** Long enough for a working state to be seen. */
const pause = (ms = 500): Promise<void> =>
  new Promise((resolve) => window.setTimeout(resolve, ms));

/** People the preview can invite, and the cards their pictures come from. */
const PEOPLE: readonly { id: string; name: string; art: string }[] = [
  { id: "sol", name: "Sol", art: "Krenko, Mob Boss" },
  { id: "emberfox", name: "EmberFox", art: "Goblin Guide" },
  { id: "mosswitch", name: "Mosswitch", art: "Aether Vial" },
  { id: "corin", name: "Corin", art: "Goblin Piledriver" },
];
const TABLE_RULES = ["Free-for-All", "Two-Headed Giant", "Emperor"] as const;
const INFLUENCES = ["Unlimited", "1", "2"] as const;

type DemoSeat = Omit<LobbySeat, "portrait" | "deck"> & {
  /** The card whose art is this seat's picture. */
  art: string | null;
  /** Your deck's id, "selected" for another person, or the AI's deck id or RANDOM_DECK. */
  deck: string | null;
};
type DemoRoom = {
  rules: LobbyRules;
  seats: DemoSeat[];
  phase: Phase;
  deadline: number | null;
  notice: string | null;
  departure: Departure | null;
};

const openSeat = (index: number): DemoSeat => ({
  id: `seat-${index}-${Math.random().toString(36).slice(2, 7)}`,
  kind: "open",
  name: "Open seat",
  art: null,
  isHost: false,
  isReady: false,
  deck: null,
  needsNewDeck: false,
  difficulty: null,
});
const aiSeat = (id: string): DemoSeat => ({
  id,
  kind: "ai",
  name: "AI player",
  art: null,
  isHost: false,
  isReady: true,
  deck: RANDOM_DECK,
  needsNewDeck: false,
  difficulty: "normal",
});

/** Every human goes back to Not ready, and any countdown stops: the rules changed under them. */
function resetAll(room: DemoRoom): DemoRoom {
  return {
    ...room,
    phase: room.phase === "countdown" ? "open" : room.phase,
    deadline: null,
    seats: room.seats.map((seat) =>
      seat.kind === "ai" ? seat : { ...seat, isReady: false },
    ),
  };
}

/**
 * The local preview of a friend-game lobby. You host it with the deck you
 * chose; Mira has joined, an AI player is ready and one seat is open. Others
 * act on their own: Mira readies once you do, someone you invite joins a
 * moment later, and when every seat is ready the ten-second countdown runs
 * and the game starts. Nothing reaches an account or another person.
 */
export function useDemoLobby(
  mode: PlayMode,
  deckId: string | null,
  onLeave: () => void,
  onStarted: () => void,
): LobbySource {
  const shelf = useDemoDeckShelf(mode);
  const { catalogue } = useBuilderCatalogue();
  const identity = useDemoIdentity();
  const shelfDecks = useMemo(
    () =>
      shelf.status === "ready"
        ? shelf.decks.map((deck): LobbyDeck => ({
            id: deck.id,
            name: deck.name,
            formatName: deck.format.name,
            appearance: deck.appearance,
          }))
        : [],
    [shelf],
  );
  const firstDeck =
    shelfDecks.find((deck) => deck.id === deckId) ?? shelfDecks[0] ?? null;
  const [room, setRoom] = useState<DemoRoom | null>(null);
  // The room opens once the decks are known, with the one Play was pressed for.
  useEffect(() => {
    if (room || shelf.status !== "ready") return;
    setRoom({
      rules: {
        format: firstDeck?.formatName ?? DEMO_FORMATS[0],
        tableRules: "Free-for-All",
        players: 4,
        match: "Single game",
        planechase: false,
        influence: "Unlimited",
      },
      seats: [
        {
          id: "seat-you",
          kind: "you",
          name: "You",
          art: null,
          isHost: true,
          isReady: false,
          deck: firstDeck?.id ?? null,
          needsNewDeck: false,
          difficulty: null,
        },
        {
          id: "seat-mira",
          kind: "person",
          name: "Mira",
          art: "Goblin Matron",
          isHost: false,
          isReady: false,
          deck: "selected",
          needsNewDeck: false,
          difficulty: null,
        },
        aiSeat("seat-ai"),
        openSeat(3),
      ],
      phase: "open",
      deadline: null,
      notice: null,
      departure: null,
    });
  }, [room, shelf.status, firstDeck]);

  const update = useCallback(
    (change: (current: DemoRoom) => DemoRoom) =>
      setRoom((current) => (current ? change(current) : current)),
    [],
  );
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

  // Others act on their own. Each effect reschedules when the room changes.
  const you = room?.seats.find((seat) => seat.kind === "you");
  useEffect(() => {
    if (!room || room.phase !== "open") return;
    const waiting = room.seats.filter(
      (seat) =>
        seat.kind === "person" &&
        (seat.needsNewDeck || (!seat.isReady && you?.isReady)),
    );
    const handles = waiting.map((seat, index) =>
      window.setTimeout(
        () =>
          update((current) => ({
            ...current,
            seats: current.seats.map((other) =>
              other.id !== seat.id
                ? other
                : other.needsNewDeck
                  ? { ...other, needsNewDeck: false, deck: "selected" }
                  : { ...other, isReady: true },
            ),
          })),
        2200 + index * 1300,
      ),
    );
    return () => handles.forEach((handle) => window.clearTimeout(handle));
  }, [room, you?.isReady, update]);
  // Every seat ready, every deck fitting: the countdown starts by itself.
  useEffect(() => {
    if (
      room?.phase === "open" &&
      canStart(seatsFor(room.seats)) &&
      room.seats.every((seat) => seat.isReady)
    )
      update((current) => ({
        ...current,
        phase: "countdown",
        deadline: Date.now() + COUNTDOWN_MS,
      }));
  }, [room, update]);
  // The deadline passes: the start commits, then the game begins.
  useEffect(() => {
    if (room?.phase !== "countdown" || room.deadline === null) return;
    const handle = window.setTimeout(
      () => {
        update((current) => ({
          ...current,
          phase: "starting",
          deadline: null,
        }));
        later(STARTING_MS, () => {
          update((current) => ({ ...current, phase: "started" }));
          onStarted();
        });
      },
      Math.max(0, room.deadline - Date.now()),
    );
    return () => window.clearTimeout(handle);
  }, [room?.phase, room?.deadline, update, later, onStarted]);

  const artFor = useCallback(
    (name: string | null): Portrait | null => {
      const card = name ? catalogue?.find(name) : undefined;
      return card ? { artUrl: card.artUrl, crop: CENTRED } : null;
    },
    [catalogue],
  );
  /** Seats as the screen shows them: your deck by name, others' private. */
  function seatsFor(seats: readonly DemoSeat[]): LobbySeat[] {
    return seats.map((seat) => ({
      ...seat,
      portrait: seat.kind === "you" ? identity.portrait : artFor(seat.art),
      deck:
        seat.deck === null
          ? null
          : seat.deck === "selected"
            ? "selected"
            : seat.deck === RANDOM_DECK
              ? "random"
              : (shelfDecks.find((deck) => deck.id === seat.deck) ?? null),
    }));
  }

  if (!room) return { status: "loading" };
  const seats = seatsFor(room.seats);
  const yourSeat = seats.find((seat) => seat.kind === "you");
  const yourDeck =
    yourSeat && typeof yourSeat.deck === "object" ? yourSeat.deck : null;
  const isLocked = room.phase !== "open";
  const eligible = shelfDecks.filter(
    (deck) => deck.formatName === room.rules.format,
  );
  const hostChange = async (
    change: (current: DemoRoom) => DemoRoom,
  ): Promise<Outcome> => {
    if (isLocked) return failed("The game is starting.");
    await pause(350);
    update((current) => resetAll(change(current)));
    return succeeded(undefined);
  };

  return {
    status: "ready",
    mode,
    rules: room.rules,
    seats,
    isHost: true,
    yourDeck,
    phase: room.phase,
    deadline: room.deadline,
    notice: room.notice,
    dismissNotice: () => update((current) => ({ ...current, notice: null })),
    decks: { state: "ready", items: eligible },
    reloadDecks: () => undefined,
    chooseDeck: async (id) => {
      if (isLocked) return failed("The game is starting.");
      if (!eligible.some((deck) => deck.id === id))
        return failed("Choose a deck for this format.");
      await pause();
      // Only your own Ready resets when your deck changes.
      update((current) => ({
        ...current,
        seats: current.seats.map((seat) =>
          seat.kind === "you"
            ? { ...seat, deck: id, needsNewDeck: false, isReady: false }
            : seat,
        ),
      }));
      return succeeded(undefined);
    },
    setReady: async (ready) => {
      if (room.phase === "starting" || room.phase === "started")
        return failed("The game is starting.");
      if (ready && (!yourDeck || yourSeat?.needsNewDeck))
        return failed("Choose a deck first.");
      await pause(250);
      update((current) => ({
        ...current,
        // Not ready during the countdown stops it.
        ...(ready || current.phase !== "countdown"
          ? {}
          : { phase: "open" as const, deadline: null }),
        seats: current.seats.map((seat) =>
          seat.kind === "you" ? { ...seat, isReady: ready } : seat,
        ),
      }));
      return succeeded(undefined);
    },
    start: async () => {
      if (room.phase !== "open" || !canStart(seats))
        return failed("Fill every seat before starting.");
      await pause(250);
      update((current) => ({
        ...current,
        phase: "countdown",
        deadline: Date.now() + COUNTDOWN_MS,
      }));
      return succeeded(undefined);
    },
    checkStart: () => undefined,
    setController: (seatId, kind) =>
      hostChange((current) => ({
        ...current,
        seats: current.seats.map((seat) =>
          seat.id !== seatId || seat.kind === "you" || seat.kind === "person"
            ? seat
            : kind === "ai"
              ? aiSeat(seat.id)
              : { ...openSeat(0), id: seat.id },
        ),
      })),
    setDifficulty: (seatId, difficulty: AiDifficulty) =>
      hostChange((current) => ({
        ...current,
        seats: current.seats.map((seat) =>
          seat.id === seatId ? { ...seat, difficulty } : seat,
        ),
      })),
    aiQuota: {
      displayUnlimited: false,
      dailyLimit: 3,
      remainingToday: 3,
      requiredAdvanced: room.seats.filter(
        (seat) => seat.kind === "ai" && seat.difficulty === "advanced",
      ).length,
      resetsAt: new Date(
        Date.UTC(
          new Date().getUTCFullYear(),
          new Date().getUTCMonth(),
          new Date().getUTCDate() + 1,
        ),
      ).toISOString(),
    },
    setAiDeck: (seatId, id) =>
      hostChange((current) => ({
        ...current,
        seats: current.seats.map((seat) =>
          seat.id === seatId
            ? { ...seat, deck: id, needsNewDeck: false }
            : seat,
        ),
      })),
    rulesChoices: {
      formats: DEMO_FORMATS,
      tableRules: TABLE_RULES,
      players: [2, 3, 4, 5, 6],
      matches: ["Single game", "Best of three"],
      planechase: true,
      influences: INFLUENCES,
    },
    applyRules: async (next) => {
      const people = room.seats.filter(
        (seat) => seat.kind === "you" || seat.kind === "person",
      ).length;
      if (next.players < people)
        return failed(
          `${people} people are here. Choose ${people} or more players.`,
        );
      if (next.tableRules === "Two-Headed Giant" && next.players !== 4)
        return failed("Two-Headed Giant needs 4 players.");
      if (next.tableRules === "Emperor" && next.players !== 6)
        return failed("Emperor needs 6 players.");
      return hostChange((current) => {
        // Joined people always keep their seats; open seats go first, then AI seats.
        let seats = [...current.seats];
        while (seats.length > next.players) {
          const drop =
            seats.findLastIndex((seat) => seat.kind === "open") >= 0
              ? seats.findLastIndex((seat) => seat.kind === "open")
              : seats.findLastIndex((seat) => seat.kind === "ai");
          seats.splice(drop, 1);
        }
        while (seats.length < next.players) seats.push(openSeat(seats.length));
        if (next.format !== current.rules.format)
          seats = seats.map((seat) => {
            if (seat.kind === "person" && seat.deck)
              return { ...seat, needsNewDeck: true };
            if (seat.kind === "you" && seat.deck) {
              const deck = shelfDecks.find((owned) => owned.id === seat.deck);
              return {
                ...seat,
                needsNewDeck: deck?.formatName !== next.format,
              };
            }
            if (seat.kind === "ai" && seat.deck !== RANDOM_DECK) {
              const deck = shelfDecks.find((owned) => owned.id === seat.deck);
              return {
                ...seat,
                needsNewDeck: deck?.formatName !== next.format,
              };
            }
            return seat;
          });
        return { ...current, rules: next, seats };
      });
    },
    findPlayers: async (query) => {
      await pause(400);
      const needle = query.trim().toLowerCase();
      const here = new Set(room.seats.map((seat) => seat.name));
      return PEOPLE.filter(
        (person) =>
          !here.has(person.name) && person.name.toLowerCase().includes(needle),
      ).map((person): InviteCandidate => ({
        id: person.id,
        name: person.name,
        portrait: artFor(person.art),
      }));
    },
    invite: async (playerId) => {
      const person = PEOPLE.find((candidate) => candidate.id === playerId);
      if (!person || !room.seats.some((seat) => seat.kind === "open"))
        return failed("Couldn’t send invitation.");
      await pause(600);
      update((current) => ({
        ...current,
        notice: `Invitation sent to ${person.name}`,
      }));
      // An invitation reserves nothing: they take whatever human seat is open when they arrive.
      later(2000, () =>
        update((current) => {
          const index = current.seats.findIndex((seat) => seat.kind === "open");
          if (index < 0 || current.phase !== "open") return current;
          const seats = [...current.seats];
          const seat = seats[index];
          if (!seat) return current;
          seats[index] = {
            ...seat,
            kind: "person",
            name: person.name,
            art: person.art,
            deck: "selected",
          };
          return { ...current, seats };
        }),
      );
      return succeeded(undefined);
    },
    inviteLink: null,
    departure: room.departure,
    beginDeparture: () =>
      update((current) => ({ ...current, departure: { stage: "confirm" } })),
    cancelDeparture: () =>
      update((current) => ({ ...current, departure: null })),
    confirmDeparture: () => {
      update((current) => ({ ...current, departure: { stage: "pending" } }));
      later(900, onLeave);
    },
    checkDeparture: () => undefined,
  };
}
