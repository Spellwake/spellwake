import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { useAtomValue } from "jotai";
import {
  identifierSchema,
  matchDetailSchema,
  type PlayerCursor,
  type PlayerSnapshot,
  type RoomGameHandoff,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "../data/account";
import { scryfallImage } from "../data/scryfall-images";
import type { CastOffer } from "./cast-draft";
import {
  gameResult,
  gameRow,
  looksFinished,
  type GameRow,
} from "./game-result";
import {
  gameRows,
  ownDecisions,
  seriesStanding,
  type SeriesStanding,
} from "./match-series";
import { matchPreferences, readPreferences } from "./table-panels";
import type {
  MatchActionCommand,
  MatchChatEntry,
  MatchConnectionState,
  MatchSeries,
  MatchSource,
} from "./match-sources";

type ActiveGameTarget = {
  matchId: string | null;
  gameId: string;
  contextId: string;
};

function isUuid(value: string | null | undefined): value is string {
  return typeof value === "string" && identifierSchema.safeParse(value).success;
}

/**
 * Connects `/match` to the live Spellwake game server:
 * - Resolves `(matchId, gameId, contextId)` from direct URL params, a `?room=`
 *   lobby handoff (`/v1/rooms/{room}/game`), or the account's latest active match.
 * - Loads initial snapshot via `/v1/games/{game}/view`, then subscribes to
 *   `/v1/games/{game}/stream` with automatic cursor reconnect (`/reconnect`).
 * - Resolves card art via `/v1/catalogue/metadata` (Scryfall URLs), seat names,
 *   and match chat.
 */
export function useConnectedMatch(
  params: {
    gameId?: string | null;
    contextId?: string | null;
    matchId?: string | null;
    roomId?: string | null;
  },
  onLeave: () => void,
): MatchSource {
  const { state, services } = useAccount();
  // Kept the same from one render to the next: the live connection below
  // restarts whenever the owner or the game it follows changes.
  const session = state.status === "ready" ? state.session : null;
  const owner = useMemo(
    () =>
      session?.accountId ? { ...session, accountId: session.accountId } : null,
    [session],
  );

  const directTarget = useMemo(
    (): ActiveGameTarget | null =>
      isUuid(params.gameId) && isUuid(params.contextId)
        ? {
            matchId: isUuid(params.matchId) ? params.matchId : null,
            gameId: params.gameId,
            contextId: params.contextId,
          }
        : null,
    [params.gameId, params.contextId, params.matchId],
  );

  // Resolve from room handoff when arriving from Lobby.
  const roomHandoff = useQuery<RoomGameHandoff | null>({
    queryKey: ["match-room-handoff", owner?.accountId, params.roomId],
    enabled: !!owner && !!services && !directTarget && isUuid(params.roomId),
    refetchInterval: (query) => {
      const data = query.state.data;
      if (data?.state === "admitted" && data.game?.contextId) return false;
      return 1500;
    },
    queryFn: async ({ signal }): Promise<RoomGameHandoff | null> => {
      if (!owner || !services || !isUuid(params.roomId)) return null;
      const res = await services.api.roomGame(
        await ownerBearer(services, owner),
        params.roomId,
        signal,
      );
      return res.status === "success" ? res.data : null;
    },
  });

  // Fallback: discover the account's latest active match/game when visiting `/match` directly.
  const latestMatch = useQuery({
    queryKey: ["match-latest-active", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services && !directTarget && !isUuid(params.roomId),
    queryFn: async ({ signal }): Promise<ActiveGameTarget | null> => {
      if (!owner || !services) return null;
      const bearer = await ownerBearer(services, owner);
      const listRes = await services.api.matches(bearer, null, 20, signal);
      if (listRes.status !== "success") return null;
      const items = Array.isArray(listRes.data["items"])
        ? (listRes.data["items"] as Array<Record<string, unknown>>)
        : [];
      const active =
        items.find(
          (m) => m["state"] === "active" || m["state"] === "in_progress",
        ) ?? items[0];
      const matchId =
        active && typeof active["id"] === "string" ? active["id"] : null;
      if (!matchId) return null;
      const gamesRes = await services.api.matchGames(
        bearer,
        matchId,
        null,
        10,
        signal,
      );
      if (gamesRes.status !== "success") return null;
      const games = Array.isArray(gamesRes.data["items"])
        ? (gamesRes.data["items"] as Array<Record<string, unknown>>)
        : [];
      const game =
        games.find(
          (g) => g["state"] === "active" && typeof g["contextId"] === "string",
        ) ?? games.find((g) => typeof g["contextId"] === "string");
      if (
        !game ||
        typeof game["id"] !== "string" ||
        typeof game["contextId"] !== "string"
      )
        return null;
      return { matchId, gameId: game["id"], contextId: game["contextId"] };
    },
  });

  // The game this page arrived at. A match of several games moves on from it.
  const arrived: ActiveGameTarget | null = useMemo(() => {
    if (directTarget) return directTarget;
    if (
      roomHandoff.data?.state === "admitted" &&
      roomHandoff.data.game?.contextId
    ) {
      return {
        matchId: roomHandoff.data.matchId,
        gameId: roomHandoff.data.game.id,
        contextId: roomHandoff.data.game.contextId,
      };
    }
    return latestMatch.data ?? null;
  }, [directTarget, roomHandoff.data, latestMatch.data]);
  const [followed, setFollowed] = useState<ActiveGameTarget | null>(null);
  const target =
    followed && followed.matchId === arrived?.matchId ? followed : arrived;

  const [snapshot, setSnapshot] = useState<PlayerSnapshot | null>(null);
  const [connectionState, setConnectionState] =
    useState<MatchConnectionState>("connecting");
  const [commandPending, setCommandPending] = useState(false);
  const [commandError, setCommandError] = useState<string | null>(null);
  const cursorRef = useRef<PlayerCursor | null>(null);

  // The game's stream only carries the table. How a game came out is in the
  // match's own record, read once the table shows there is something to read.
  const tableLooksFinished = snapshot !== null && looksFinished(snapshot);
  const finishedGame = useQuery<GameRow | null>({
    queryKey: [
      "match-game-result",
      owner?.accountId,
      target?.matchId,
      target?.gameId,
    ],
    enabled: !!owner && !!services && !!target?.matchId && snapshot !== null,
    refetchInterval: (query) =>
      query.state.data
        ? false
        : tableLooksFinished || connectionState !== "live"
          ? 2000
          : 12000,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !target?.matchId) return null;
      const res = await services.api.matchGames(
        await ownerBearer(services, owner),
        target.matchId,
        null,
        10,
        signal,
      );
      if (res.status !== "success" || !Array.isArray(res.data["items"]))
        return null;
      const row = gameRow(res.data["items"], target.gameId);
      return row && row.state !== "reserved" && row.state !== "active"
        ? row
        : null;
    },
  });
  const result = useMemo(
    () =>
      finishedGame.data && snapshot
        ? gameResult(finishedGame.data, snapshot)
        : null,
    [finishedGame.data, snapshot],
  );
  const finished = result !== null;

  // One game of several: where the match stands, and what the next game
  // waits on. Read again until the match is decided or the next game is dealt.
  const seatCount = snapshot?.players.length ?? 0;
  const standingQuery = useQuery<{
    standing: SeriesStanding | null;
    settled: boolean;
  }>({
    queryKey: [
      "match-series",
      owner?.accountId,
      target?.matchId,
      target?.gameId,
      seatCount,
    ],
    enabled: finished && !!owner && !!services && !!target?.matchId,
    refetchInterval: (query) => (query.state.data?.settled ? false : 2000),
    queryFn: async ({ signal }) => {
      const unread = { standing: null, settled: false };
      if (!owner || !services || !target?.matchId) return unread;
      const bearer = await ownerBearer(services, owner);
      const [detailRes, gamesRes, decisionsRes] = await Promise.all([
        services.api.matchDetail(bearer, target.matchId, signal),
        services.api.matchGames(bearer, target.matchId, null, 10, signal),
        services.api.matchDecisions(bearer, target.matchId, null, 20, signal),
      ]);
      const detail =
        detailRes.status === "success"
          ? matchDetailSchema.safeParse(detailRes.data)
          : null;
      if (!detail?.success || gamesRes.status !== "success") return unread;
      if (detail.data.match.structure === "bo1")
        return { standing: null, settled: true };
      const items = (data: Record<string, unknown>) =>
        Array.isArray(data["items"]) ? (data["items"] as unknown[]) : [];
      const standing = seriesStanding(
        detail.data,
        gameRows(items(gamesRes.data)),
        decisionsRes.status === "success"
          ? ownDecisions(items(decisionsRes.data))
          : [],
        target.gameId,
        seatCount,
      );
      return { standing, settled: standing?.next.kind === "over" };
    },
  });
  const standing = standingQuery.data?.standing ?? null;

  // The next game has been dealt: this page follows the match to it.
  const started = standing?.next.kind === "started" ? standing.next : null;
  const matchId = target?.matchId ?? null;
  useEffect(() => {
    if (!started || !matchId) return;
    cursorRef.current = null;
    setSnapshot(null);
    setConnectionState("connecting");
    setFollowed({
      matchId,
      gameId: started.gameId,
      contextId: started.contextId,
    });
  }, [started, matchId]);

  // Starting the next game at once means answering with the deck already
  // being played. The room this match came from is where that deck is named.
  const deciding = standing?.next.kind === "deciding" ? standing.next : null;
  const playedDeck = useQuery({
    queryKey: ["match-played-deck", owner?.accountId, params.roomId],
    enabled: !!deciding && !!owner && !!services && isUuid(params.roomId),
    staleTime: Infinity,
    queryFn: async ({ signal }): Promise<string | null> => {
      if (!owner || !services || !isUuid(params.roomId)) return null;
      const res = await services.api.roomEnrichedDetail(
        await ownerBearer(services, owner),
        params.roomId,
        signal,
      );
      if (res.status !== "success") return null;
      return (
        res.data.slots.find((slot) => slot.accountId === owner.accountId)
          ?.deckRevisionId ?? null
      );
    },
  });
  const [answer, setAnswer] = useState<{
    decisionId: string;
    state: "pending" | "failed";
  } | null>(null);
  const deckRevisionId = playedDeck.data ?? null;
  const refetchStanding = standingQuery.refetch;
  const ready = useCallback(async () => {
    if (!owner || !services || !deciding || !deckRevisionId) return;
    setAnswer({ decisionId: deciding.decisionId, state: "pending" });
    let taken = false;
    try {
      const res = await services.api.betweenGameReply(
        await ownerBearer(services, owner),
        {
          requestId: crypto.randomUUID(),
          decisionId: deciding.decisionId,
          expectedRevision: deciding.matchRevision,
          selectedRevisionId: deckRevisionId,
        },
      );
      taken = res.status === "success";
    } catch {
      taken = false;
    }
    if (taken) await refetchStanding();
    // Not taken, the time still runs and the same deck is kept at its end.
    setAnswer(
      taken ? null : { decisionId: deciding.decisionId, state: "failed" },
    );
  }, [owner, services, deciding, deckRevisionId, refetchStanding]);

  const series = useMemo((): MatchSeries | null => {
    if (!standing) return null;
    const { gameNumber, score, next } = standing;
    if (next.kind === "over") return { gameNumber, score, next };
    if (next.kind !== "deciding")
      return { gameNumber, score, next: { kind: "waiting" } };
    const mine = answer?.decisionId === next.decisionId ? answer.state : null;
    return {
      gameNumber,
      score,
      next: {
        kind: "deciding",
        deadlineAt: next.deadlineAt,
        ready: deckRevisionId ? () => void ready() : null,
        readyPending: mine === "pending",
        readyFailed: mine === "failed",
      },
    };
  }, [standing, answer, deckRevisionId, ready]);

  // Initial view load + live SSE stream with automatic reconnect.
  useEffect(() => {
    // A finished game has nothing more to send.
    if (!owner || !services || !target || finished) return;
    const abort = new AbortController();
    let stopped = false;
    setConnectionState("connecting");

    const runLoop = async () => {
      let attempt = 0;
      while (!stopped && !abort.signal.aborted) {
        try {
          const bearer = await ownerBearer(services, owner);
          if (!cursorRef.current) {
            const view = await services.api.gameView(
              bearer,
              target.gameId,
              target.contextId,
              abort.signal,
            );
            if (stopped || abort.signal.aborted) return;
            if (view.status === "success") {
              setSnapshot(view.data);
              setConnectionState("live");
              attempt = 0;
            }
          } else {
            const rec = await services.api.gameReconnect(
              bearer,
              target.gameId,
              target.contextId,
              cursorRef.current,
              abort.signal,
            );
            if (stopped || abort.signal.aborted) return;
            if (rec.status === "success") {
              if ("snapshot" in rec.data && rec.data.snapshot) {
                setSnapshot(rec.data.snapshot);
              }
              if ("cursor" in rec.data && rec.data.cursor) {
                cursorRef.current = rec.data.cursor;
              }
            }
          }

          setConnectionState("live");
          const streamRes = await services.api.gameStream(
            bearer,
            target.gameId,
            target.contextId,
            cursorRef.current,
            (frame) => {
              if (frame.kind === "snapshot") {
                setSnapshot(frame.snapshot);
                cursorRef.current = frame.cursor;
                setConnectionState("live");
              } else if (frame.kind === "gap") {
                cursorRef.current = frame.cursor;
              }
            },
            abort.signal,
          );
          if (stopped || abort.signal.aborted) return;
          if (streamRes.status === "fail") {
            setConnectionState("ended");
            return;
          }
        } catch {
          if (stopped || abort.signal.aborted) return;
        }
        attempt += 1;
        setConnectionState("reconnecting");
        await new Promise((resolve) =>
          window.setTimeout(resolve, Math.min(5000, 800 * attempt)),
        );
      }
    };

    void runLoop();
    return () => {
      stopped = true;
      abort.abort();
    };
  }, [owner, services, target, finished]);

  // Resolve seat usernames from match seats.
  const seatsQuery = useQuery({
    queryKey: ["match-seats", owner?.accountId, target?.matchId],
    enabled: !!owner && !!services && !!target?.matchId,
    staleTime: 60_000,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !target?.matchId) return [];
      const res = await services.api.matchSeats(
        await ownerBearer(services, owner),
        target.matchId,
        null,
        12,
        signal,
      );
      if (res.status !== "success") return [];
      return Array.isArray(res.data["items"])
        ? (res.data["items"] as Array<Record<string, unknown>>)
        : [];
    },
  });

  const seatNames = useMemo(() => {
    const map = new Map<string, string>();
    const players = snapshot?.players ?? [];
    players.forEach((p, idx) => {
      map.set(
        p.seatId,
        p.seatId === snapshot?.viewerSeatId ? "You" : `Player ${idx + 1}`,
      );
    });
    for (const raw of seatsQuery.data ?? []) {
      const seatId =
        typeof raw["seatId"] === "string"
          ? raw["seatId"]
          : typeof raw["id"] === "string"
            ? raw["id"]
            : null;
      if (!seatId) continue;
      if (seatId === snapshot?.viewerSeatId) {
        map.set(seatId, "You");
      } else if (
        typeof raw["username"] === "string" &&
        raw["username"].trim()
      ) {
        map.set(seatId, raw["username"]);
      } else if (raw["controllerKind"] === "ai" || raw["isAi"] === true) {
        map.set(seatId, "Spellwake AI");
      }
    }
    return map;
  }, [snapshot?.players, snapshot?.viewerSeatId, seatsQuery.data]);

  // Resolve Scryfall artwork for all visible printings in the game.
  const printingIds = useMemo(() => {
    const ids = new Set<string>();
    for (const obj of snapshot?.visibleObjects ?? []) {
      if (obj.identity.kind === "known" && obj.identity.printingId) {
        ids.add(obj.identity.printingId);
      }
    }
    return [...ids].sort();
  }, [snapshot?.visibleObjects]);

  const printingChunks = useMemo(() => {
    const chunks: string[][] = [];
    for (let i = 0; i < printingIds.length; i += 50) {
      chunks.push(printingIds.slice(i, i + 50));
    }
    return chunks;
  }, [printingIds]);

  const artQueries = useQueries({
    queries: printingChunks.map((chunk) => ({
      queryKey: ["match-card-art", owner?.accountId, chunk.join(",")],
      enabled: !!owner && !!services && chunk.length > 0,
      staleTime: Infinity,
      queryFn: async ({ signal }: { signal: AbortSignal }) => {
        if (!owner || !services) return [];
        const res = await services.api.cardMetadata(
          await ownerBearer(services, owner),
          { printingIds: chunk },
          signal,
        );
        if (res.status !== "success") return [];
        return res.data.printings.flatMap((p) => {
          const url =
            scryfallImage(p.externalPrintingRef, "normal") ?? p.imageUrl;
          return url ? ([[p.printingId, url]] as const) : [];
        });
      },
    })),
    combine: (results) => new Map(results.flatMap((r) => r.data ?? [])),
  });

  // Match chat polling when matchId is available.
  const chatQuery = useQuery({
    queryKey: ["match-chat", owner?.accountId, target?.matchId],
    enabled: !!owner && !!services && !!target?.matchId,
    // A last word after the game still arrives.
    refetchInterval: connectionState === "live" || finished ? 3000 : false,
    queryFn: async ({ signal }): Promise<MatchChatEntry[]> => {
      if (!owner || !services || !target?.matchId) return [];
      const res = await services.api.matchChat(
        await ownerBearer(services, owner),
        target.matchId,
        null,
        signal,
      );
      if (res.status !== "success") return [];
      const rawItems = Array.isArray(res.data["items"])
        ? (res.data["items"] as Array<Record<string, unknown>>)
        : [];
      return rawItems
        .filter(
          (item) =>
            typeof item["id"] === "string" && typeof item["body"] === "string",
        )
        .map((item) => {
          const senderId =
            typeof item["senderId"] === "string" ? item["senderId"] : "";
          const isMine = senderId === owner.accountId;
          return {
            id: item["id"] as string,
            senderId,
            senderName: isMine
              ? "You"
              : typeof item["senderUsername"] === "string"
                ? item["senderUsername"]
                : "Player",
            body: item["body"] as string,
            sentAt:
              typeof item["sentAt"] === "string"
                ? item["sentAt"]
                : new Date().toISOString(),
            isMine,
          };
        })
        .sort((a, b) => a.sentAt.localeCompare(b.sentAt));
    },
  });

  const sendChat = useCallback(
    async (body: string): Promise<boolean> => {
      if (!owner || !services || !target?.matchId) return false;
      const trimmed = body.trim();
      if (!trimmed) return false;
      try {
        const res = await services.api.sendMatchChat(
          await ownerBearer(services, owner),
          target.matchId,
          { requestId: crypto.randomUUID(), body: trimmed },
        );
        if (res.status === "success") {
          void chatQuery.refetch();
          return true;
        }
        return false;
      } catch {
        return false;
      }
    },
    [owner, services, target?.matchId, chatQuery],
  );

  const autoTap = readPreferences(useAtomValue(matchPreferences)).autoTap;
  const castOffers = useMemo((): readonly CastOffer[] | undefined => {
    if (!snapshot || snapshot.castOffers === undefined) return undefined;
    return snapshot.castOffers.map((offer) => ({
      source: offer.source,
      targets: offer.targets ?? null,
      cost: offer.cost,
      payment: autoTap ? "automatic" : "paused",
      paySources: offer.paySources ?? [],
    }));
  }, [snapshot, autoTap]);

  const sendAction = useCallback(
    async (action: MatchActionCommand) => {
      if (!owner || !services || !target || !snapshot || commandPending) return;
      setCommandPending(true);
      setCommandError(null);
      try {
        const bearer = await ownerBearer(services, owner);
        let current = snapshot;
        let finalAction: MatchActionCommand = action;

        // When casting a server-offered spell with payment: null, execute the
        // server-authored mana-source taps first (never rolling back accepted taps),
        // then submit the cast with the exact server-computed payment.
        if (
          (action.kind === "cast-untargeted" ||
            action.kind === "cast-targeted") &&
          action.payment === null
        ) {
          const offer = current.castOffers?.find(
            (entry) => entry.source === action.source,
          );
          if (offer) {
            for (const paySource of offer.paySources ?? []) {
              const permanent = current.visibleObjects.find(
                (obj) => obj.handle === paySource,
              );
              const manaAbility = permanent?.abilities.find(
                (ab) => ab.kind === "mana",
              );
              if (!permanent || !manaAbility) continue;
              const tapRes = await services.api.gameCommand(
                bearer,
                target.gameId,
                {
                  requestId: crypto.randomUUID(),
                  basis: current.basis,
                  action: {
                    kind: "activate-mana",
                    source: paySource,
                    ability: manaAbility.handle,
                  },
                },
              );
              const refreshed = await services.api.gameView(
                bearer,
                target.gameId,
                target.contextId,
              );
              if (refreshed.status === "success") {
                current = refreshed.data;
                setSnapshot(refreshed.data);
              }
              if (tapRes.status === "fail") {
                const msg =
                  Object.values(tapRes.data).find(
                    (v) => typeof v === "string",
                  ) ?? "Mana activation was not accepted.";
                setCommandError(msg);
                return;
              }
              if (tapRes.status === "error") {
                setCommandError("Couldn’t reach the game server.");
                return;
              }
            }
            if (offer.payment) {
              finalAction = { ...action, payment: offer.payment };
            }
          }
        }

        const res = await services.api.gameCommand(bearer, target.gameId, {
          requestId: crypto.randomUUID(),
          basis: current.basis,
          action: finalAction,
        });
        if (res.status === "fail") {
          const msg =
            Object.values(res.data).find((v) => typeof v === "string") ??
            "Action was not accepted.";
          setCommandError(msg);
          // Refresh view in case basis moved.
          const fresh = await services.api.gameView(
            bearer,
            target.gameId,
            target.contextId,
          );
          if (fresh.status === "success") setSnapshot(fresh.data);
        } else if (res.status === "error") {
          setCommandError("Couldn’t reach the game server.");
        } else {
          // Immediately refresh view so responsiveness feels instant even between SSE frames.
          const fresh = await services.api.gameView(
            bearer,
            target.gameId,
            target.contextId,
          );
          if (fresh.status === "success") setSnapshot(fresh.data);
        }
      } catch {
        setCommandError("Couldn’t send action.");
      } finally {
        setCommandPending(false);
      }
    },
    [owner, services, target, snapshot, commandPending],
  );

  const chooseOpening = useCallback(
    async (playFirst: boolean) => {
      if (!owner || !services || !target) return;
      setCommandPending(true);
      setCommandError(null);
      try {
        const res = await services.api.gameOpeningChoice(
          await ownerBearer(services, owner),
          target.gameId,
          { gameId: target.gameId, playFirst },
        );
        if (res.status !== "success") {
          setCommandError("Couldn’t submit opening choice.");
        }
      } catch {
        setCommandError("Couldn’t reach the game server.");
      } finally {
        setCommandPending(false);
      }
    },
    [owner, services, target],
  );

  if (state.status === "signed_out") return { status: "signed-out" };
  if (state.status === "loading") return { status: "loading" };
  if (state.status !== "ready") return { status: "unavailable" };
  if (state.session.state === "needs_registration")
    return { status: "unregistered" };

  if (!target) {
    if (roomHandoff.isPending || latestMatch.isPending)
      return { status: "loading" };
    return { status: "idle" };
  }
  if (!snapshot) return { status: "loading" };

  return {
    status: "ready",
    matchId: target.matchId,
    gameId: target.gameId,
    snapshot,
    seatNames,
    artByPrintingId: artQueries,
    ...(castOffers !== undefined ? { castOffers } : {}),
    connectionState,
    result,
    series,
    commandPending,
    commandError,
    dismissCommandError: () => setCommandError(null),
    sendAction,
    chooseOpening: snapshot.turn === null ? chooseOpening : null,
    chat: {
      messages: chatQuery.data ?? [],
      send: target.matchId ? sendChat : null,
    },
    onLeave,
  };
}
