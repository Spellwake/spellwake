import { useEffect, useMemo, useState } from "react";
import { keepPreviousData, useQueries, useQuery } from "@tanstack/react-query";
import {
  matchDetailSchema,
  type PlayerVisibleEvent,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { useInventoryGap } from "../collection/connected-inventory";
import { ownerBearer } from "../data/account";
import { useDeckFormats } from "../data/deck-commands";
import { scryfallImage } from "../data/scryfall-images";
import { seatRows } from "../league/history-read";
import { opponentsLine } from "../league/history-sources";
import { gameRows } from "./match-series";
import {
  historyQuery,
  nextTurn,
  previousTurn,
  replayRequest,
  timeline,
} from "./replay";
import type { ReplaySource } from "./replay-sources";

/** The most history pages read for one game: ten thousand events. */
const MOST_PAGES = 100;
const items = (data: Record<string, unknown>): unknown[] =>
  Array.isArray(data["items"]) ? data["items"] : [];

/**
 * A finished match of the account's, replayed. The match names its games
 * and seats; a game's history lists its events, read from the start; and
 * each step's table is read as it is reached, from the player's own seat or,
 * where the game allows it, with every hand shown.
 */
export function useConnectedReplay(
  matchId: string,
  gameIndex: number,
  chooseGame: (index: number) => void,
): ReplaySource {
  const gap = useInventoryGap();
  const { state, services } = useAccount();
  const session = state.status === "ready" ? state.session : null;
  const owner = useMemo(
    () =>
      session?.accountId ? { ...session, accountId: session.accountId } : null,
    [session],
  );
  const formats = useDeckFormats();
  const read = !!owner && !!services;

  const match = useQuery({
    queryKey: ["replay-match", owner?.accountId, matchId],
    enabled: read,
    staleTime: Infinity,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to replay.");
      const bearer = await ownerBearer(services, owner);
      const [detail, seats, games] = await Promise.all([
        services.api.matchDetail(bearer, matchId, signal),
        services.api.matchSeats(bearer, matchId, null, 12, signal),
        services.api.matchGames(bearer, matchId, null, 10, signal),
      ]);
      if (
        detail.status !== "success" ||
        seats.status !== "success" ||
        games.status !== "success"
      )
        throw new Error("Match unavailable.");
      return {
        detail: matchDetailSchema.parse(detail.data),
        seats: seatRows(items(seats.data)),
        // Only a finished game with a table can be replayed.
        games: gameRows(items(games.data)).filter(
          (game) =>
            game.contextId !== null &&
            (game.state === "completed" || game.state === "no_contest"),
        ),
      };
    },
  });
  const game = match.data?.games[gameIndex] ?? null;
  const ownSeat = match.data?.detail.ownSeatId ?? null;

  const history = useQuery({
    queryKey: ["replay-history", owner?.accountId, game?.id],
    enabled: read && !!game?.contextId,
    staleTime: Infinity,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !game?.contextId) return [];
      const bearer = await ownerBearer(services, owner);
      const events: PlayerVisibleEvent[] = [];
      let cursor = null;
      for (let page = 0; page < MOST_PAGES; page += 1) {
        const query = historyQuery({
          gameId: game.id,
          contextId: game.contextId,
          cursor,
        });
        if (!query) break;
        const result = await services.api.gameHistory(bearer, query, signal);
        if (result.status !== "success")
          throw new Error("History unavailable.");
        events.push(...result.data.events);
        if (!result.data.hasMore) break;
        cursor = result.data.scannedThrough;
      }
      return events;
    },
  });
  const line = useMemo(() => timeline(history.data ?? []), [history.data]);

  const [step, setStep] = useState(0);
  const [everyHand, setEveryHand] = useState(false);
  const [everyHandOffered, setEveryHandOffered] = useState(true);
  // A new game starts from its first step.
  useEffect(() => setStep(0), [game?.id]);
  const sequence = line.steps[step] ?? null;

  const frame = useQuery({
    queryKey: [
      "replay-frame",
      owner?.accountId,
      game?.id,
      sequence,
      everyHand && everyHandOffered,
    ],
    enabled: read && !!game?.contextId && !!ownSeat && sequence !== null,
    staleTime: Infinity,
    // The table stays on screen while the next step is read.
    placeholderData: keepPreviousData,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !game?.contextId || !ownSeat || !sequence)
        return null;
      const full = everyHand && everyHandOffered;
      const request = replayRequest({
        gameId: game.id,
        contextId: game.contextId,
        atEventSequence: sequence,
        perspectiveSeatId: ownSeat,
        everyHand: full,
      });
      if (!request) return null;
      const result = await services.api.gameReplay(
        await ownerBearer(services, owner),
        request,
        signal,
      );
      if (result.status === "success") return result.data.snapshot;
      // A game that cannot show every hand is replayed from the player's seat.
      if (full && result.status === "fail" && "mode" in result.data)
        setEveryHandOffered(false);
      throw new Error("Step unavailable.");
    },
  });

  const printingIds = useMemo(() => {
    const ids = new Set<string>();
    for (const object of frame.data?.visibleObjects ?? [])
      if (object.identity.kind === "known" && object.identity.printingId)
        ids.add(object.identity.printingId);
    return [...ids].sort();
  }, [frame.data]);
  const art = useQueries({
    queries: Array.from(
      { length: Math.ceil(printingIds.length / 50) },
      (_, page) => printingIds.slice(page * 50, page * 50 + 50),
    ).map((chunk) => ({
      queryKey: ["match-card-art", owner?.accountId, chunk.join(",")],
      enabled: read && chunk.length > 0,
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

  const seatNames = useMemo(
    () =>
      new Map(
        (match.data?.seats ?? []).map((seat) => [
          seat.id,
          seat.id === ownSeat
            ? "You"
            : (seat.person?.username ?? "Spellwake AI"),
        ]),
      ),
    [match.data, ownSeat],
  );

  if (gap) return { status: gap };
  if (match.isPending) return { status: "loading" };
  if (!match.data) return { status: "unavailable" };
  if (match.data.games.length === 0) return { status: "missing" };
  const format =
    (formats.data?.pages ?? [])
      .flatMap((page) => page.items)
      .find((item) => item.id === match.data.detail.match.formatId)?.name ??
    null;
  const others = match.data.seats
    .filter((seat) => seat.id !== ownSeat)
    .map((seat) => seat.person?.username ?? "Spellwake AI");
  const last = Math.max(0, line.steps.length - 1);
  return {
    status: "ready",
    title: [`vs ${opponentsLine(others)}`, ...(format ? [format] : [])].join(
      " · ",
    ),
    games: match.data.games.map((entry, index) => ({
      id: entry.id,
      label: `Game ${index + 1}`,
    })),
    gameIndex: Math.min(gameIndex, match.data.games.length - 1),
    chooseGame,
    seatNames,
    faceOf: (object) =>
      object.identity.kind === "known" && object.identity.printingId
        ? art.get(object.identity.printingId)
        : undefined,
    snapshot: frame.data ?? null,
    step,
    steps: history.isPending ? 0 : line.steps.length,
    go: (to) => setStep(Math.max(0, Math.min(last, to))),
    nextTurn: () => setStep((at) => nextTurn(line, at)),
    previousTurn: () => setStep((at) => previousTurn(line, at)),
    everyHand: everyHandOffered ? everyHand : null,
    setEveryHand,
    notice:
      history.isError || frame.isError
        ? "Part of this game couldn’t be read."
        : !history.isPending && line.steps.length === 0
          ? "This game has nothing to replay."
          : null,
  };
}
