import { useCallback, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { parseAsString, useQueryState } from "nuqs";
import { useNavigate } from "react-router-dom";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer, useDeckLibrary } from "../data/account";
import { useDeckFormats } from "../data/deck-commands";
import type {
  ActiveQueue,
  RankedDeckChoice,
  SeasonFormatRank,
  SeasonQuest,
  SeasonSource,
  StandingRow,
  StandingsSource,
} from "./league-sources";

function tierEmblem(tier: string): { name: string; emblemUrl: string } {
  const clean = tier.trim() || "Unranked";
  const lower = clean.toLowerCase();
  const known = [
    "unranked",
    "bronze",
    "silver",
    "gold",
    "platinum",
    "diamond",
    "mythic",
  ];
  const slug = known.includes(lower) ? lower : "unranked";
  return {
    name: clean.charAt(0).toUpperCase() + clean.slice(1).toLowerCase(),
    emblemUrl: `${import.meta.env.BASE_URL}ranks/${slug}.svg`,
  };
}

function tierBounds(rating: number): {
  lower: number;
  next: number;
  highest: boolean;
  label: string;
} {
  if (rating >= 2000) {
    return { lower: 2000, next: 2000, highest: true, label: "Mythic" };
  }
  const step = 200;
  const lower = Math.floor(Math.max(800, rating) / step) * step;
  const next = lower + step;
  return {
    lower,
    next,
    highest: false,
    label: `${rating} / ${next}`,
  };
}

function formatDateRange(startsAt: unknown, endsAt: unknown): string | null {
  if (typeof startsAt !== "string" || typeof endsAt !== "string") return null;
  const start = new Date(startsAt);
  const end = new Date(endsAt);
  if (!Number.isFinite(start.getTime()) || !Number.isFinite(end.getTime()))
    return null;
  const fmt = (d: Date) =>
    d.toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
      timeZone: "UTC",
    });
  return `${fmt(start)} – ${fmt(end)}`;
}

export function useConnectedSeason(): SeasonSource {
  const { state, services } = useAccount();
  const queries = useQueryClient();
  const navigate = useNavigate();
  const formatsQuery = useDeckFormats();
  const decksQuery = useDeckLibrary();
  const [queueBusy, setQueueBusy] = useState(false);
  const [rerollingQuestId, setRerollingQuestId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const owner =
    state.status === "ready" && state.session.accountId
      ? { ...state.session, accountId: state.session.accountId }
      : null;

  const seasonsQuery = useQuery({
    queryKey: ["league-seasons", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    queryFn: async ({ signal }) => {
      if (!owner || !services) return [];
      const res = await services.api.seasons(
        await ownerBearer(services, owner),
        null,
        20,
        signal,
      );
      if (res.status !== "success") return [];
      return Array.isArray(res.data["items"])
        ? (res.data["items"] as Array<Record<string, unknown>>)
        : [];
    },
  });

  const activeSeason = useMemo(() => {
    const items = seasonsQuery.data ?? [];
    return items.find((s) => s["state"] === "active") ?? items[0] ?? null;
  }, [seasonsQuery.data]);
  const activeSeasonId =
    activeSeason && typeof activeSeason["id"] === "string"
      ? activeSeason["id"]
      : null;

  const ranksQuery = useQuery({
    queryKey: ["league-ranks", owner?.accountId, activeSeasonId],
    enabled: !!owner && !!services,
    queryFn: async ({ signal }) => {
      if (!owner || !services) return [];
      const res = await services.api.ranks(
        await ownerBearer(services, owner),
        { ...(activeSeasonId ? { seasonId: activeSeasonId } : {}), limit: 20 },
        signal,
      );
      if (res.status !== "success") return [];
      return Array.isArray(res.data["items"])
        ? (res.data["items"] as Array<Record<string, unknown>>)
        : [];
    },
  });

  const questsQuery = useQuery({
    queryKey: ["league-quests", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    queryFn: async ({ signal }) => {
      if (!owner || !services) return [];
      const res = await services.api.quests(
        await ownerBearer(services, owner),
        null,
        20,
        signal,
      );
      if (res.status !== "success") return [];
      return Array.isArray(res.data["items"])
        ? (res.data["items"] as Array<Record<string, unknown>>)
        : [];
    },
  });

  const queueQuery = useQuery<Array<Record<string, unknown>>>({
    queryKey: ["league-queue", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    refetchInterval: (query) => {
      const items = query.state.data ?? [];
      return items.some(
        (q) => q["state"] === "queued" || q["state"] === "searching",
      )
        ? 2500
        : false;
    },
    queryFn: async ({ signal }): Promise<Array<Record<string, unknown>>> => {
      if (!owner || !services) return [];
      const res = await services.api.queueState(
        await ownerBearer(services, owner),
        null,
        10,
        signal,
      );
      if (res.status !== "success") return [];
      const items = Array.isArray(res.data["items"])
        ? (res.data["items"] as Array<Record<string, unknown>>)
        : [];
      const matched = items.find(
        (q) => q["state"] === "matched" && typeof q["matchId"] === "string",
      );
      if (matched && typeof matched["matchId"] === "string") {
        void navigate(`/match?match=${encodeURIComponent(matched["matchId"])}`);
      }
      return items;
    },
  });

  const formats = useMemo(
    () =>
      (formatsQuery.data?.pages ?? []).flatMap((p) =>
        p.items.map((f) => ({ id: f.id, name: f.name })),
      ),
    [formatsQuery.data],
  );
  const formatById = useMemo(
    () => new Map(formats.map((f) => [f.id, f.name] as const)),
    [formats],
  );

  const decks: RankedDeckChoice[] = useMemo(
    () =>
      (decksQuery.data?.pages ?? []).flatMap((p) =>
        p.items.map((d) => ({
          id: d.id,
          name: d.name,
          formatId: d.format.id,
          formatName: d.format.name,
          revisionId: d.currentRevisionId,
        })),
      ),
    [decksQuery.data],
  );

  const ranks: SeasonFormatRank[] = useMemo(
    () =>
      (ranksQuery.data ?? []).map((raw) => {
        const formatId =
          typeof raw["formatId"] === "string" ? raw["formatId"] : "";
        const rating = Number(raw["rating"] ?? 1000) || 1000;
        const emblem = tierEmblem(
          typeof raw["tier"] === "string" ? raw["tier"] : "unranked",
        );
        const bounds = tierBounds(rating);
        return {
          formatId,
          formatName: formatById.get(formatId) ?? "Format",
          tier: emblem.name,
          emblemUrl: emblem.emblemUrl,
          rating,
          lowerBound: bounds.lower,
          nextBound: bounds.next,
          isHighest: bounds.highest,
          progressLabel: bounds.label,
          wins: Number(raw["wins"] ?? 0) || 0,
          losses: Number(raw["losses"] ?? 0) || 0,
          draws: Number(raw["draws"] ?? 0) || 0,
        };
      }),
    [ranksQuery.data, formatById],
  );

  const quests: SeasonQuest[] = useMemo(
    () =>
      (questsQuery.data ?? [])
        .filter((q) => typeof q["id"] === "string")
        .map((q) => ({
          id: q["id"] as string,
          title:
            typeof q["title"] === "string"
              ? q["title"]
              : typeof q["description"] === "string"
                ? q["description"]
                : "Daily quest",
          progress: Number(q["progress"] ?? 0) || 0,
          target: Math.max(1, Number(q["target"] ?? 1) || 1),
          rewardCoins: Number(q["rewardCoins"] ?? 500) || 500,
          canReroll: q["canReroll"] !== false && q["state"] !== "completed",
        })),
    [questsQuery.data],
  );

  const activeQueue: ActiveQueue | null = useMemo(() => {
    const row = (queueQuery.data ?? []).find(
      (q) =>
        q["state"] === "queued" ||
        q["state"] === "searching" ||
        q["state"] === "pending",
    );
    if (!row || typeof row["id"] !== "string") return null;
    const fId = typeof row["formatId"] === "string" ? row["formatId"] : "";
    return {
      entryId: row["id"],
      formatName: formatById.get(fId) ?? "Ranked",
      state: String(row["state"] ?? "queued"),
    };
  }, [queueQuery.data, formatById]);

  const joinQueue = useCallback(
    async (formatId: string, deckId: string) => {
      if (!owner || !services || queueBusy) return;
      const deck = decks.find((d) => d.id === deckId);
      if (!deck?.revisionId) {
        setNotice("Save your deck before entering the queue.");
        return;
      }
      setQueueBusy(true);
      setNotice(null);
      try {
        const res = await services.api.enqueueMatchmaking(
          await ownerBearer(services, owner),
          {
            requestId: crypto.randomUUID(),
            formatId,
            deckRevisionId: deck.revisionId,
          },
        );
        if (res.status === "success") {
          await queries.invalidateQueries({
            queryKey: ["league-queue", owner.accountId],
          });
        } else {
          setNotice("Couldn’t join ranked queue right now.");
        }
      } catch {
        setNotice("Couldn’t reach the server.");
      } finally {
        setQueueBusy(false);
      }
    },
    [owner, services, queueBusy, decks, queries],
  );

  const cancelQueue = useCallback(async () => {
    if (!owner || !services || !activeQueue || queueBusy) return;
    setQueueBusy(true);
    try {
      const res = await services.api.cancelMatchmaking(
        await ownerBearer(services, owner),
        activeQueue.entryId,
        { requestId: crypto.randomUUID() },
      );
      if (res.status === "success") {
        await queries.invalidateQueries({
          queryKey: ["league-queue", owner.accountId],
        });
      } else {
        setNotice("Couldn’t cancel queue entry.");
      }
    } catch {
      setNotice("Couldn’t reach the server.");
    } finally {
      setQueueBusy(false);
    }
  }, [owner, services, activeQueue, queueBusy, queries]);

  const rerollQuest = useCallback(
    async (questId: string) => {
      if (!owner || !services || rerollingQuestId) return;
      setRerollingQuestId(questId);
      try {
        const res = await services.api.rerollQuest(
          await ownerBearer(services, owner),
          { requestId: crypto.randomUUID(), assignmentId: questId },
        );
        if (res.status === "success") {
          await queries.invalidateQueries({
            queryKey: ["league-quests", owner.accountId],
          });
        } else {
          setNotice("Couldn’t reroll quest.");
        }
      } catch {
        setNotice("Couldn’t reach the server.");
      } finally {
        setRerollingQuestId(null);
      }
    },
    [owner, services, rerollingQuestId, queries],
  );

  if (state.status === "signed_out") return { status: "signed-out" };
  if (state.status === "loading") return { status: "loading" };
  if (state.status !== "ready") return { status: "unavailable" };
  if (state.session.state === "needs_registration")
    return { status: "unregistered" };

  return {
    status: "ready",
    seasonName:
      activeSeason && typeof activeSeason["name"] === "string"
        ? activeSeason["name"]
        : activeSeason && typeof activeSeason["code"] === "string"
          ? activeSeason["code"]
          : "Current Season",
    seasonDates: activeSeason
      ? formatDateRange(activeSeason["startsAt"], activeSeason["endsAt"])
      : null,
    ranks,
    quests,
    decks,
    formats,
    activeQueue,
    queueBusy,
    rerollingQuestId,
    notice,
    dismissNotice: () => setNotice(null),
    joinQueue,
    cancelQueue,
    rerollQuest,
  };
}

export function useConnectedStandings(): StandingsSource {
  const { state, services } = useAccount();
  const formatsQuery = useDeckFormats();
  const [selectedFormat, setSelectedFormat] = useQueryState(
    "format",
    parseAsString.withDefault(""),
  );
  const [, setPlayer] = useQueryState("player", parseAsString);

  const owner =
    state.status === "ready" && state.session.accountId
      ? { ...state.session, accountId: state.session.accountId }
      : null;

  const formats = useMemo(
    () =>
      (formatsQuery.data?.pages ?? []).flatMap((p) =>
        p.items.map((f) => ({ id: f.id, name: f.name })),
      ),
    [formatsQuery.data],
  );
  const formatId = selectedFormat || formats[0]?.id || "";

  const leaderboardQuery = useQuery({
    queryKey: ["league-leaderboard", owner?.accountId, formatId],
    enabled: !!owner && !!services,
    queryFn: async ({ signal }) => {
      if (!owner || !services) return null;
      const res = await services.api.leagueLeaderboard(
        await ownerBearer(services, owner),
        { ...(formatId ? { formatId } : {}), limit: 50 },
        signal,
      );
      if (res.status !== "success") throw new Error("Standings unavailable.");
      return res.data;
    },
  });

  if (state.status === "signed_out") return { status: "signed-out" };
  if (state.status === "loading") return { status: "loading" };
  if (state.status !== "ready") return { status: "unavailable" };
  if (state.session.state === "needs_registration")
    return { status: "unregistered" };

  const rows: StandingRow[] = (leaderboardQuery.data?.items ?? []).map(
    (entry) => {
      const emblem = tierEmblem(entry.tier);
      return {
        rankPosition: entry.rankPosition,
        accountId: entry.accountId,
        username: entry.username,
        tier: emblem.name,
        emblemUrl: emblem.emblemUrl,
        rating: Number(entry.rating) || 1000,
        wins: entry.wins,
        losses: entry.losses,
        draws: entry.draws,
        supporter: entry.supporter,
      };
    },
  );

  return {
    status: "ready",
    formats,
    selectedFormatId: formatId,
    onSelectFormat: (id) => void setSelectedFormat(id || null),
    seasonName: null,
    rows,
    total: leaderboardQuery.data?.total ?? 0,
    isLoadingRows: leaderboardQuery.isPending,
    error: leaderboardQuery.isError ? "Couldn’t load standings." : null,
    onOpenProfile: (id) => void setPlayer(id),
  };
}
