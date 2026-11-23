import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useStore } from "jotai";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  addAiErrorsSchema,
  bootstrapAckSchema,
  bootstrapDispositionSchema,
  bootstrapErrorsSchema,
  bootstrapRequestSchema,
  commandAckSchema,
  configureRoomSchema,
  forceStartDispositionSchema,
  forceStartErrorsSchema,
  readyRoomDispositionSchema,
  readyRoomErrorsSchema,
  removeAiDispositionSchema,
  removeAiErrorsSchema,
  revisionRequestSchema,
  roomCommandDispositionSchema,
  roomCommandErrorsSchema,
  roomCommandSchema,
  roomConfigurationAckSchema,
  roomConfigurationDispositionSchema,
  roomConfigurationErrorsSchema,
  setAiTierDispositionSchema,
  setAiTierErrorsSchema,
  setAiTierRequestSchema,
  unreadyDispositionSchema,
  unreadyErrorsSchema,
  addAiDispositionSchema,
  type RoomCommand,
} from "@spellwake/application-contracts";
import { DEFAULT_APPEARANCE } from "@spellwake/deck-box";
import { z } from "zod";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer, useDeckLibrary } from "../data/account";
import { useDeckFormats } from "../data/deck-commands";
import type { IntentDisposition } from "../data/intent-journal";
import {
  failed,
  succeeded,
  type Outcome,
  type PlayMode,
} from "../decks/deck-sources";
import { useInventoryGap } from "../collection/connected-inventory";
import {
  MATCH_LABEL,
  type Departure,
  type LobbyDeck,
  type LobbyRules,
  type LobbySeat,
  type LobbySource,
  type Phase,
} from "./lobby-sources";
import {
  runRoomAction,
  type RoomActionKind,
  type RoomOutcome,
} from "./room-intents";

const POLL_MS = 2000;
const READ_ATTEMPTS = 3;
const COULDNT = "Couldn’t change the lobby.";
const UNCONFIRMED = "Couldn’t confirm the change.";
type Errors = Record<string, string>;
const asErrors = (schema: z.ZodType) => schema as z.ZodType<Errors>;
const asDisposition = (schema: z.ZodType) =>
  schema as z.ZodType<IntentDisposition<Errors>>;

/** Table rules by their profile key, as written for people. */
const PROFILE_NAMES: Record<string, string> = {
  free_for_all: "Free-for-All",
  "free-for-all": "Free-for-All",
  two_headed_giant: "Two-Headed Giant",
  "two-headed-giant": "Two-Headed Giant",
  emperor: "Emperor",
};
const profileName = (key: string): string =>
  PROFILE_NAMES[key] ??
  key
    .split(/[_-]+/)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
/** Why a refused action was refused, said briefly. */
function refusal(errors: Errors): string {
  if ("aiQuota" in errors)
    return "You’ve used all your Advanced AI games for today.";
  if ("revision" in errors) return "The lobby changed. Try again.";
  if ("deck" in errors || "decks" in errors)
    return "Choose a deck for this format.";
  if ("capacity" in errors) return "The lobby is full.";
  if ("account" in errors) return "This account can’t do that now.";
  return COULDNT;
}

/**
 * The account's friend-game lobby. The room is read every two seconds; each
 * action is one journaled command against the revision the screen last read,
 * so a stale action is refused rather than applied to a changed room. The
 * server cannot set AI difficulty, give an AI a chosen deck, change a room's
 * format or invite a named person, so those controls are left out.
 */
export function useConnectedLobby(
  mode: PlayMode,
  roomId: string | null,
  deckId: string | null,
  onRoom: (roomId: string) => void,
  onLeave: () => void,
  onStarted: () => void,
): LobbySource {
  const gap = useInventoryGap();
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  const store = useStore();
  const queries = useQueryClient();
  const library = useDeckLibrary();
  const formats = useDeckFormats();
  const [notice, setNotice] = useState<string | null>(null);
  const [departure, setDeparture] = useState<Departure | null>(null);
  const [isStarting, setIsStarting] = useState<
    "starting" | "unconfirmed" | null
  >(null);
  const [createFailed, setCreateFailed] = useState(false);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = library;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  useEffect(() => {
    if (formats.hasNextPage && !formats.isFetchingNextPage)
      void formats.fetchNextPage();
  }, [formats]);

  const ownDecks = useMemo(
    () =>
      (library.data?.pages ?? [])
        .flatMap((page) => page.items)
        .filter((deck) => deck.mode === mode),
    [library.data, mode],
  );
  const formatNames = useMemo(
    () =>
      new Map(
        (formats.data?.pages ?? []).flatMap((page) =>
          page.items.map((format) => [format.id, format.name] as const),
        ),
      ),
    [formats.data],
  );

  /** The API, for a command's send and lookup; they only run once signed in. */
  const api = useCallback(() => {
    if (!services) throw new Error("Sign in to play.");
    return services.api;
  }, [services]);
  const bearer = useCallback(async () => {
    if (!owner || !services) throw new Error("Sign in to play.");
    return ownerBearer(services, owner);
  }, [owner, services]);
  const roomKey = ["room", owner?.accountId, owner?.sessionId, roomId];
  const detail = useQuery({
    queryKey: roomKey,
    enabled: !!owner && !!services && !!roomId,
    refetchInterval: POLL_MS,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !roomId) throw new Error("No room.");
      const result = await services.api.roomDetail(
        await bearer(),
        owner.accountId,
        roomId,
        signal,
      );
      if (result.status === "fail") return null;
      if (result.status !== "success") throw new Error("Room unavailable.");
      return result.data;
    },
  });
  const profiles = useQuery({
    queryKey: ["table-profiles", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    staleTime: Infinity,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to play.");
      const result = await services.api.tableProfiles(
        await bearer(),
        owner.accountId,
        signal,
      );
      if (result.status !== "success") throw new Error("Rules unavailable.");
      return result.data.items;
    },
  });
  const room = detail.data?.room ?? null;
  const isHost = !!room && room.hostId === owner?.accountId;
  const configuration = useQuery({
    queryKey: ["room-configuration", owner?.accountId, roomId, room?.revision],
    enabled: !!owner && !!services && !!roomId && isHost,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !roomId) throw new Error("No room.");
      const result = await services.api.roomConfiguration(
        await bearer(),
        owner.accountId,
        roomId,
        signal,
      );
      if (result.status !== "success") throw new Error("Rules unavailable.");
      return result.data;
    },
  });
  const hasAiSeats = (detail.data?.slots.items ?? []).some(
    (slot) => slot.controllerKind === "ai",
  );
  const aiStatus = useQuery({
    queryKey: ["room-ai", owner?.accountId, roomId, room?.revision],
    enabled: !!owner && !!services && !!roomId && hasAiSeats,
    refetchInterval: POLL_MS,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !roomId) throw new Error("No room.");
      const result = await services.api.roomAiStatus(
        await bearer(),
        owner.accountId,
        roomId,
        signal,
      );
      if (result.status !== "success") return null;
      return result.data;
    },
  });
  const aiQuotaQuery = useQuery({
    queryKey: ["ai-quota", owner?.accountId],
    enabled: !!owner && !!services && isHost,
    staleTime: 30_000,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to play.");
      const result = await services.api.aiQuota(await bearer(), signal);
      if (result.status !== "success") return null;
      return result.data;
    },
  });
  const others = (detail.data?.slots.items ?? []).flatMap((slot) =>
    slot.accountId && slot.accountId !== owner?.accountId
      ? [slot.accountId]
      : [],
  );
  const names = useQueries({
    queries: others.map((accountId) => ({
      queryKey: ["player-name", owner?.accountId, accountId],
      enabled: !!owner && !!services,
      staleTime: 60_000,
      retry: READ_ATTEMPTS - 1,
      queryFn: async ({ signal }: { signal: AbortSignal }) => {
        if (!owner || !services) throw new Error("Sign in to play.");
        const result = await services.api.playerProfile(
          await bearer(),
          owner.accountId,
          accountId,
          signal,
        );
        if (result.status !== "success") throw new Error("Name unavailable.");
        return [accountId, result.data.person.username] as const;
      },
    })),
    combine: (results) =>
      new Map(results.flatMap((result) => (result.data ? [result.data] : []))),
  });

  /** One journaled action, settled against the room's current revision. */
  const act = useCallback(
    async <Command, Ack>(
      kind: RoomActionKind,
      target: string | null,
      spec: {
        commandSchema: z.ZodType<Command>;
        requestIdOf?: (command: Command) => string;
        ackSchema: z.ZodType<Ack>;
        errorsSchema: z.ZodType;
        dispositionSchema: z.ZodType;
        build: (() => Command) | null;
        send: (bearer: string, command: Command) => Promise<unknown>;
        lookup: (bearer: string, command: Command) => Promise<unknown>;
      },
    ) => {
      if (!owner || !services) return { state: "unknown" } as const;
      return runRoomAction<Command, Ack>({
        store,
        origin: services.api.origin,
        accountId: owner.accountId,
        kind,
        target,
        commandSchema: spec.commandSchema,
        requestIdOf:
          spec.requestIdOf ??
          ((command) => (command as { requestId: string }).requestId),
        ackSchema: spec.ackSchema,
        errorsSchema: asErrors(spec.errorsSchema),
        dispositionSchema: asDisposition(spec.dispositionSchema),
        build: spec.build,
        send: async (command) =>
          (await spec.send(await bearer(), command)) as Awaited<
            ReturnType<typeof services.api.roomCommand>
          >,
        lookup: async (command) =>
          (await spec.lookup(await bearer(), command)) as Awaited<
            ReturnType<typeof services.api.roomCommandDisposition>
          > as never,
        refresh: async () => {
          if (!isCurrentSession(owner))
            throw new Error("Your session changed. Reopen the lobby.");
          await queries.invalidateQueries({
            queryKey: ["room", owner.accountId],
          });
        },
      });
    },
    [owner, services, store, bearer, isCurrentSession, queries],
  );
  const roomCommand = useCallback(
    (
      kind: "create" | "select_deck" | "leave",
      target: string | null,
      build: (() => RoomCommand) | null,
    ) =>
      act<RoomCommand, z.infer<typeof commandAckSchema>>(kind, target, {
        commandSchema: roomCommandSchema as z.ZodType<RoomCommand>,
        requestIdOf: (command) => command.command.requestId,
        ackSchema: commandAckSchema,
        errorsSchema: roomCommandErrorsSchema,
        dispositionSchema: roomCommandDispositionSchema,
        build,
        send: (token, command) => api().roomCommand(token, command),
        lookup: (token, command) =>
          api().roomCommandDisposition(token, command),
      }),
    [act, api],
  );

  // Play with a deck and no room yet: the room is made once, then the deck chosen in it.
  const creating = useRef(false);
  useEffect(() => {
    if (roomId || creating.current || !owner || !services) return;
    const deck = ownDecks.find((candidate) => candidate.id === deckId);
    const profile =
      profiles.data?.find((p) => p.key.includes("free")) ?? profiles.data?.[0];
    if (!deck || !profile) return;
    creating.current = true;
    void roomCommand("create", null, () => ({
      kind: "create",
      command: {
        requestId: crypto.randomUUID(),
        formatId: deck.format.id,
        profileId: profile.id,
        mode,
        competitive: false,
        visibility: "private",
      },
    })).then((outcome) => {
      if (outcome.state === "accepted") onRoom(outcome.ack.resourceId);
      else setCreateFailed(true);
    });
  }, [
    roomId,
    owner,
    services,
    ownDecks,
    deckId,
    profiles.data,
    mode,
    roomCommand,
    onRoom,
  ]);
  // A new room takes the deck Play was pressed for, once its revision is known.
  const choseFirst = useRef(false);
  const chooseDeck = useCallback(
    async (id: string): Promise<Outcome> => {
      const deck = ownDecks.find((candidate) => candidate.id === id);
      if (!room || !roomId || !deck?.currentRevisionId)
        return failed("Choose a saved deck.");
      if (deck.format.id !== room.formatId)
        return failed("Choose a deck for this format.");
      const revision = deck.currentRevisionId;
      const outcome = await roomCommand("select_deck", roomId, () => ({
        kind: "select_deck",
        command: {
          requestId: crypto.randomUUID(),
          roomId,
          expectedRevision: room.revision,
          deckRevisionId: revision,
        },
      }));
      return outcome.state === "accepted"
        ? succeeded(undefined)
        : failed(
            outcome.state === "rejected"
              ? refusal(outcome.errors)
              : UNCONFIRMED,
          );
    },
    [ownDecks, room, roomId, roomCommand],
  );
  useEffect(() => {
    if (choseFirst.current || !room || !deckId || !detail.data) return;
    const mine = detail.data.slots.items.find(
      (s) => s.accountId === owner?.accountId,
    );
    if (!mine || mine.deckRevisionId) return;
    choseFirst.current = true;
    void chooseDeck(deckId);
  }, [room, deckId, detail.data, owner?.accountId, chooseDeck]);

  /** Ready, Not ready, force start or add an AI seat: one journaled action on this room. */
  const control = useCallback(
    (
      action: "ready" | "unready" | "force-start" | "ai",
      recoverOnly = false,
    ) => {
      if (!room || !roomId)
        return Promise.resolve({ state: "unknown" } as const);
      const kind: RoomActionKind =
        action === "force-start"
          ? "force_start"
          : action === "ai"
            ? "add_ai"
            : action;
      const schemas = {
        ready: [readyRoomErrorsSchema, readyRoomDispositionSchema],
        unready: [unreadyErrorsSchema, unreadyDispositionSchema],
        "force-start": [forceStartErrorsSchema, forceStartDispositionSchema],
        ai: [addAiErrorsSchema, addAiDispositionSchema],
      } as const;
      const [errorsSchema, dispositionSchema] = schemas[action];
      return act(kind, roomId, {
        commandSchema: revisionRequestSchema,
        ackSchema: roomConfigurationAckSchema,
        errorsSchema,
        dispositionSchema,
        build: recoverOnly
          ? null
          : () => ({
              requestId: crypto.randomUUID(),
              expectedRevision: room.revision,
            }),
        send: (token, command) =>
          api().roomControl(token, roomId, action, command),
        lookup: (token, command) =>
          api().roomControlDisposition(token, roomId, action, command),
      });
    },
    [room, roomId, act, api],
  );

  // The room says how things stand; a started game opens the match.
  const startedOnce = useRef(false);
  useEffect(() => {
    if (room?.state === "started" && !startedOnce.current) {
      startedOnce.current = true;
      onStarted();
    }
    if (room?.state === "countdown" || room?.state === "started")
      setIsStarting(null);
  }, [room?.state, onStarted]);

  const slots = useMemo(
    () =>
      [...(detail.data?.slots.items ?? [])].sort(
        (a, b) => Number(a.seatIndex) - Number(b.seatIndex),
      ),
    [detail.data],
  );
  const lobbyDeck = (deck: (typeof ownDecks)[number]): LobbyDeck => ({
    id: deck.id,
    name: deck.name,
    formatName: deck.format.name,
    appearance: {
      ...DEFAULT_APPEARANCE,
      faceArt: deck.faceCard?.artUrl ?? null,
    },
  });

  if (gap === "loading") return { status: "loading" };
  if (gap)
    return {
      status:
        gap === "signed-out"
          ? "signed-out"
          : gap === "unregistered"
            ? "unregistered"
            : "unavailable",
    };
  if (!roomId)
    return createFailed ? { status: "unavailable" } : { status: "loading" };
  if (detail.isPending) return { status: "loading" };
  if (detail.data === null || detail.isError) return { status: "unavailable" };
  if (!detail.data || !room) return { status: "loading" };
  if (room.state === "closed") return { status: "closed" };

  const roomFormat = formatNames.get(room.formatId) ?? "This format";
  const profile = profiles.data?.find((p) => p.id === room.profileId) ?? null;
  const seatCount = Number(room.targetSeatCount ?? slots.length);
  const aiTierBySlot = new Map(
    (aiStatus.data?.items ?? []).map((item) => [item.slotId, item.aiTier]),
  );
  const seats: LobbySeat[] = slots.map((slot) => {
    if (slot.controllerKind === "ai")
      return {
        id: slot.id,
        kind: "ai",
        name: "AI player",
        portrait: null,
        isHost: false,
        isReady: slot.ready,
        deck: "random",
        needsNewDeck: false,
        difficulty: aiTierBySlot.get(slot.id) ?? "normal",
      };
    const isYou = slot.accountId === owner?.accountId;
    const own = isYou
      ? ownDecks.find((deck) => deck.currentRevisionId === slot.deckRevisionId)
      : undefined;
    return {
      id: slot.id,
      kind: isYou ? "you" : "person",
      name: isYou ? "You" : (names.get(slot.accountId ?? "") ?? "Player"),
      portrait: null,
      isHost: slot.accountId === room.hostId,
      isReady: slot.ready,
      deck: isYou
        ? own
          ? lobbyDeck(own)
          : slot.deckRevisionId
            ? "selected"
            : null
        : "private",
      needsNewDeck: !!own && own.format.id !== room.formatId,
      difficulty: null,
    };
  });
  for (let index = seats.length; index < seatCount; index += 1)
    seats.push({
      id: `open-${index}`,
      kind: "open",
      name: "Open seat",
      portrait: null,
      isHost: false,
      isReady: false,
      deck: null,
      needsNewDeck: false,
      difficulty: null,
    });
  const yourSeat = seats.find((seat) => seat.kind === "you");
  const rules: LobbyRules = {
    format: roomFormat,
    tableRules: profile ? profileName(profile.key) : "Table rules",
    players: seatCount,
    match: MATCH_LABEL[room.structure],
    planechase: null,
    influence: null,
  };
  const phase: Phase =
    room.state === "started"
      ? "started"
      : isStarting === "unconfirmed"
        ? "start-unconfirmed"
        : isStarting === "starting"
          ? "starting"
          : room.state === "countdown"
            ? "countdown"
            : "open";
  const eligible = ownDecks
    .filter(
      (deck) => deck.format.id === room.formatId && deck.currentRevisionId,
    )
    .map(lobbyDeck);
  const settle = (outcome: RoomOutcome<unknown>): Outcome =>
    outcome.state === "accepted"
      ? succeeded(undefined)
      : failed(
          outcome.state === "rejected" ? refusal(outcome.errors) : UNCONFIRMED,
        );
  const leave = (recoverOnly: boolean) => {
    setDeparture(
      recoverOnly
        ? { stage: "unconfirmed", isChecking: true }
        : { stage: "pending" },
    );
    void roomCommand(
      "leave",
      roomId,
      recoverOnly
        ? null
        : () => ({
            kind: "leave",
            command: {
              requestId: crypto.randomUUID(),
              roomId,
              expectedRevision: room.revision,
            },
          }),
    ).then((outcome) => {
      if (outcome.state === "accepted") onLeave();
      else if (outcome.state === "rejected")
        setDeparture({ stage: "rejected" });
      else setDeparture({ stage: "unconfirmed", isChecking: false });
    });
  };
  const startGame = async (recoverOnly: boolean): Promise<Outcome> => {
    setIsStarting("starting");
    const outcome = await control("force-start", recoverOnly);
    if (outcome.state === "unknown") setIsStarting("unconfirmed");
    else if (outcome.state === "rejected") setIsStarting(null);
    return settle(outcome);
  };

  return {
    status: "ready",
    mode,
    rules,
    seats,
    isHost,
    yourDeck:
      yourSeat && typeof yourSeat.deck === "object" ? yourSeat.deck : null,
    phase,
    deadline: room.countdownAt ? Date.parse(room.countdownAt) : null,
    notice,
    dismissNotice: () => setNotice(null),
    decks: {
      state: library.isError
        ? "failed"
        : library.isPending
          ? "loading"
          : "ready",
      items: eligible,
    },
    reloadDecks: () => void library.refetch(),
    chooseDeck,
    setReady: async (ready) =>
      settle(await control(ready ? "ready" : "unready")),
    start: isHost ? () => startGame(false) : null,
    checkStart: () => void startGame(true),
    setController: isHost
      ? async (seatId, kind) => {
          if (kind === "ai") {
            const before = new Set(
              slots.filter((s) => s.controllerKind === "ai").map((s) => s.id),
            );
            const added = await control("ai");
            if (added.state !== "accepted") return settle(added);
            // The new AI seat is prepared with Random Deck before it can be ready.
            const fresh = await detail.refetch();
            const seat = fresh.data?.slots.items.find(
              (s) => s.controllerKind === "ai" && !before.has(s.id),
            );
            const revision = fresh.data?.room.revision;
            if (!seat || !revision) return failed(UNCONFIRMED);
            const prepared = await act("bootstrap_ai", seat.id, {
              commandSchema: bootstrapRequestSchema,
              ackSchema: bootstrapAckSchema,
              errorsSchema: bootstrapErrorsSchema,
              dispositionSchema: bootstrapDispositionSchema,
              build: () => ({
                requestId: crypto.randomUUID(),
                expectedRevision: revision,
                deckRevisionId: null,
              }),
              send: (token, command) =>
                api().bootstrapAi(token, seat.id, command),
              lookup: (token, command) =>
                api().bootstrapAiDisposition(token, seat.id, command),
            });
            return settle(prepared);
          }
          const slot = slots.find((candidate) => candidate.id === seatId);
          if (!slot || slot.controllerKind !== "ai") return failed(COULDNT);
          const outcome = await act("remove_ai", slot.id, {
            commandSchema: revisionRequestSchema,
            ackSchema: z.strictObject({ resourceId: z.string() }),
            errorsSchema: removeAiErrorsSchema,
            dispositionSchema: removeAiDispositionSchema,
            build: () => ({
              requestId: crypto.randomUUID(),
              expectedRevision: room.revision,
            }),
            send: (token, command) => api().removeAi(token, slot.id, command),
            lookup: (token, command) =>
              api().removeAiDisposition(token, slot.id, command),
          });
          return settle(outcome);
        }
      : null,
    setDifficulty: isHost
      ? async (seatId, tier) => {
          const outcome = await act("set_ai_tier", seatId, {
            commandSchema: setAiTierRequestSchema,
            ackSchema: roomConfigurationAckSchema,
            errorsSchema: setAiTierErrorsSchema,
            dispositionSchema: setAiTierDispositionSchema,
            build: () => ({
              requestId: crypto.randomUUID(),
              expectedRevision: room.revision,
              tier,
            }),
            send: (token, command) => api().setAiTier(token, seatId, command),
            lookup: (token, command) =>
              api().setAiTierDisposition(token, seatId, command),
          });
          if (outcome.state === "accepted" && owner) {
            await queries.invalidateQueries({
              queryKey: ["room-ai", owner.accountId, roomId],
            });
          }
          return settle(outcome);
        }
      : null,
    aiQuota: aiQuotaQuery.data
      ? {
          displayUnlimited: aiQuotaQuery.data.displayUnlimited,
          dailyLimit: aiQuotaQuery.data.dailyLimit,
          remainingToday: aiQuotaQuery.data.remainingToday,
          requiredAdvanced: seats.filter(
            (seat) => seat.kind === "ai" && seat.difficulty === "advanced",
          ).length,
          resetsAt: aiQuotaQuery.data.resetsAt,
        }
      : null,
    setAiDeck: null,
    rulesChoices:
      isHost && configuration.data && profiles.data
        ? {
            formats: [roomFormat],
            tableRules: profiles.data
              .filter(
                (p) =>
                  Number(p.minimumSeats) <= slots.length &&
                  Number(p.seatCapacity) >=
                    slots.filter((s) => s.accountId).length,
              )
              .map((p) => profileName(p.key)),
            players: [seatCount],
            matches: ["Single game", "Best of three"],
            planechase: false,
            influences: [],
          }
        : null,
    applyRules:
      isHost && configuration.data && profiles.data
        ? async (next) => {
            const chosen = profiles.data.find(
              (p) => profileName(p.key) === next.tableRules,
            );
            const config = configuration.data;
            if (!chosen || !config) return failed(COULDNT);
            const outcome = await act("configure", roomId, {
              commandSchema: configureRoomSchema,
              ackSchema: roomConfigurationAckSchema,
              errorsSchema: roomConfigurationErrorsSchema,
              dispositionSchema: roomConfigurationDispositionSchema,
              build: () => ({
                requestId: crypto.randomUUID(),
                expectedRevision: room.revision,
                profileId: chosen.id,
                structure: (next.match === "Best of three" ? "bo3" : "bo1") as
                  "bo1" | "bo3",
                teams: config.teams,
              }),
              send: (token, command) =>
                api().configureRoom(token, roomId, command),
              lookup: (token, command) =>
                api().configureRoomDisposition(token, roomId, command),
            });
            return settle(outcome);
          }
        : null,
    findPlayers: null,
    invite: null,
    departure,
    beginDeparture: () => setDeparture({ stage: "confirm" }),
    cancelDeparture: () => setDeparture(null),
    confirmDeparture: () => leave(false),
    checkDeparture: () => leave(true),
  };
}
