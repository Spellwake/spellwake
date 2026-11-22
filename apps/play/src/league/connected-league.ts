import { useEffect, useMemo, useState } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { z } from "zod";
import { identifierSchema } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "../data/account";
import { useDeckFormats } from "../data/deck-commands";
import { scryfallImage } from "../data/scryfall-images";
import {
  READ_ATTEMPTS,
  useInventoryGap,
} from "../collection/connected-inventory";
import {
  competitionPlaces,
  seasonOf,
  STARTING_RATING,
  type FormatRank,
  type LeagueSource,
  type PortraitData,
  type SeasonRecord,
  type StandingRow,
} from "./league-sources";

const PAGE = 50;
const RANKS_FAILED = "Couldn’t load your ranks.";
const STANDINGS_FAILED = "Couldn’t load standings.";
const MORE_FAILED = "Couldn’t load more standings.";

const seasonItem = z.object({
  id: identifierSchema,
  startAt: z.string(),
  endAt: z.string(),
  state: z.string(),
});
const rankItem = z.object({
  formatId: identifierSchema,
  seasonId: identifierSchema,
  rating: z.string(),
});
const creditItem = z.object({
  formatId: identifierSchema,
  seasonId: identifierSchema.nullable(),
});
const page = <T extends z.ZodTypeAny>(item: T) =>
  z.object({ items: z.array(item), nextCursor: z.string().nullable() });
const artProfile = z.object({
  printingId: identifierSchema,
  cropX: z.number(),
  cropY: z.number(),
  scale: z.number(),
});

function useOwner() {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  return { owner, services };
}

/** "Jan 1" from the season's end instant. */
const endsOf = (endAt: string): string =>
  new Date(endAt).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  });

/**
 * The account's League. The active season and the viewer's rank per format
 * come from `/seasons` and `/ranks`; whether a format was played this
 * season comes from its competitive results. Those results say only whether
 * each match was won, so a played format's record is shown as unavailable
 * rather than counting every other result as a loss. Standings read
 * `/league/leaderboard` fifty at a time; places are shared for equal
 * ratings across the rows read from the top. The leaderboard's win and loss
 * counts span every format and season, so they are not shown as this
 * format's record.
 */
export function useConnectedLeague(format: string | null): LeagueSource {
  const gap = useInventoryGap();
  const { owner, services } = useOwner();
  const formatsRead = useDeckFormats();
  const [notice, setNotice] = useState<string | null>(null);

  const seasons = useQuery({
    queryKey: ["league-seasons", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error(RANKS_FAILED);
      const result = await services.api.seasons(
        await ownerBearer(services, owner),
        null,
        20,
        signal,
      );
      if (result.status !== "success") throw new Error(RANKS_FAILED);
      return page(seasonItem).parse(result.data).items;
    },
  });
  const season =
    seasons.data?.find((item) => item.state === "active") ??
    seasons.data?.[0] ??
    null;

  const ranks = useQuery({
    queryKey: ["league-ranks", owner?.accountId, season?.id],
    enabled: !!owner && !!services && !!season,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !season) throw new Error(RANKS_FAILED);
      const bearer = await ownerBearer(services, owner);
      const [rankRead, creditRead] = await Promise.all([
        services.api.ranks(bearer, { seasonId: season.id, limit: 100 }, signal),
        services.api.competitiveResults(
          bearer,
          { seasonId: season.id, limit: 100 },
          signal,
        ),
      ]);
      if (rankRead.status !== "success" || creditRead.status !== "success")
        throw new Error(RANKS_FAILED);
      const played = new Set(
        page(creditItem)
          .parse(creditRead.data)
          .items.flatMap((item) =>
            item.seasonId === season.id ? [item.formatId] : [],
          ),
      );
      return {
        ratings: new Map(
          page(rankItem)
            .parse(rankRead.data)
            .items.map((item) => [item.formatId, Number(item.rating)]),
        ),
        played,
      };
    },
  });

  const formats = useMemo(
    () =>
      (formatsRead.data?.pages ?? []).flatMap((entry) =>
        entry.items.map((item) => ({ id: item.id, name: item.name })),
      ),
    [formatsRead.data],
  );
  const selected = formats.find((entry) => entry.id === format) ?? null;

  const standings = useInfiniteQuery({
    queryKey: ["league-standings", owner?.accountId, season?.id, selected?.id],
    enabled: !!owner && !!services && !!season && !!selected,
    retry: READ_ATTEMPTS - 1,
    initialPageParam: 0,
    queryFn: async ({ pageParam, signal }) => {
      if (!owner || !services || !season || !selected)
        throw new Error(STANDINGS_FAILED);
      const bearer = await ownerBearer(services, owner);
      const result = await services.api.leagueLeaderboard(
        bearer,
        {
          formatId: selected.id,
          seasonId: season.id,
          offset: pageParam,
          limit: PAGE,
        },
        signal,
      );
      if (result.status !== "success") throw new Error(STANDINGS_FAILED);
      // Each player's chosen artwork, named through the catalogue.
      const arts = result.data.items.flatMap((entry) => {
        const art = artProfile.safeParse(entry.cardArtProfile);
        return art.success ? [art.data] : [];
      });
      const printings = [...new Set(arts.map((art) => art.printingId))];
      const refs = new Map<string, string>();
      if (printings.length > 0) {
        const labels = await services.api.catalogue(
          bearer,
          { definitionIds: [], printingIds: printings },
          signal,
        );
        if (labels.status === "success")
          for (const printing of labels.data.printings)
            refs.set(printing.id, printing.externalPrintingRef);
      }
      return {
        total: result.data.total,
        offset: pageParam,
        items: result.data.items.map((entry) => {
          const art = artProfile.safeParse(entry.cardArtProfile);
          const ref = art.success ? refs.get(art.data.printingId) : undefined;
          const url = ref ? scryfallImage(ref, "art_crop") : null;
          const portrait: PortraitData | null =
            art.success && url
              ? {
                  artUrl: url,
                  crop: {
                    x: art.data.cropX,
                    y: art.data.cropY,
                    scale: art.data.scale,
                  },
                }
              : null;
          return {
            playerId: entry.accountId,
            name: entry.username,
            portrait,
            rating: Number(entry.rating),
          };
        }),
      };
    },
    getNextPageParam: (last) =>
      last.offset + last.items.length < last.total
        ? last.offset + last.items.length
        : undefined,
  });

  // A failed read leaves its notice; the screen keeps what it already has.
  const ranksFailed = seasons.isError || ranks.isError;
  const standingsFailed = standings.isError;
  const appendFailed = standingsFailed && !!standings.data;
  useEffect(() => {
    if (ranksFailed) setNotice(RANKS_FAILED);
  }, [ranksFailed]);
  useEffect(() => {
    if (standingsFailed)
      setNotice(appendFailed ? MORE_FAILED : STANDINGS_FAILED);
  }, [standingsFailed, appendFailed]);

  const rankData = ranks.data;
  const recordOf = (formatId: string): SeasonRecord =>
    rankData?.played.has(formatId) ? null : { wins: 0, losses: 0, draws: 0 };
  const ratingOf = (formatId: string): number =>
    rankData?.ratings.get(formatId) ?? STARTING_RATING;

  const loaded = useMemo(
    () => (standings.data?.pages ?? []).flatMap((entry) => entry.items),
    [standings.data],
  );
  const rows = useMemo((): StandingRow[] => {
    const places = competitionPlaces(loaded.map((row) => row.rating));
    return loaded.map((row, index) => ({
      ...row,
      place: places[index]!,
      record: null,
      isYou: row.playerId === owner?.accountId,
    }));
  }, [loaded, owner?.accountId]);

  if (gap) return { status: gap };
  if (formatsRead.isPending || seasons.isPending) return { status: "loading" };
  if (!season && !seasons.isError) return { status: "unavailable" };

  const formatRanks: FormatRank[] = formats.map((entry) => ({
    format: entry,
    rating: ratingOf(entry.id),
    record: recordOf(entry.id),
  }));
  const ownLoaded = rows.find((row) => row.isYou) ?? null;
  return {
    status: "ready",
    season: season
      ? {
          label: seasonOf(new Date(season.startAt)).label,
          ends: endsOf(season.endAt),
        }
      : seasonOf(new Date()),
    formats,
    ranks: {
      rows: rankData ? formatRanks : [],
      state: rankData
        ? ranks.isError
          ? "failed"
          : "ready"
        : ranksFailed
          ? "failed"
          : "loading",
    },
    standings: selected
      ? {
          format: selected,
          rows,
          state: standings.data
            ? "ready"
            : standingsFailed
              ? "failed"
              : "loading",
          own:
            ownLoaded ??
            (rankData && owner
              ? {
                  playerId: owner.accountId,
                  name: "You",
                  portrait: null,
                  // Not yet read down to your row, or not placed this season.
                  place: null,
                  rating: ratingOf(selected.id),
                  record: recordOf(selected.id),
                  isYou: true,
                }
              : null),
          isAppending: standings.isFetchingNextPage,
          hasMore: !!standings.hasNextPage && !appendFailed,
          loadMore: () => {
            if (standings.hasNextPage && !standings.isFetchingNextPage)
              void standings.fetchNextPage();
          },
        }
      : null,
    notice,
    dismissNotice: () => setNotice(null),
  };
}
