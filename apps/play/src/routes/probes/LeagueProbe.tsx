import type { ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { PREVIEW_FORMATS, usePreviewLeague } from "../../league/demo-league";
import type { LeagueSource, ReadyLeague } from "../../league/league-sources";
import { SeasonPage } from "../league/SeasonPage";
import { StandingsPage } from "../league/StandingsPage";

/**
 * Development only: League in states the preview cannot reach on its own,
 * for the stress checks and review captures. `?state=` names one and
 * `?page=standings` shows Standings (with `?format=`), else This season.
 */
const noop = (): void => undefined;
const MORE_FORMATS = [
  ...PREVIEW_FORMATS,
  ...[
    "Pauper",
    "Premodern",
    "Oathbreaker",
    "Brawl",
    "Historic",
    "Explorer",
  ].map((name) => ({ id: `probe-format-${name.toLowerCase()}`, name })),
];

export function LeagueProbe(): ReactElement {
  const [params] = useSearchParams();
  const state = params.get("state") ?? "ranks-loading";
  const isStandings = params.get("page") === "standings";
  const format = params.get("format") ?? PREVIEW_FORMATS[0]!.id;
  const preview = usePreviewLeague(format);
  const show = (source: LeagueSource) =>
    isStandings ? (
      <StandingsPage source={source} format={format} onFormat={noop} />
    ) : (
      <SeasonPage source={source} />
    );
  if (state === "loading" || state === "signed-out" || state === "unavailable")
    return show({ status: state });
  if (preview.status !== "ready") return show(preview);

  const over = (overrides: Partial<ReadyLeague>): LeagueSource => ({
    ...preview,
    dismissNotice: noop,
    ...overrides,
  });
  const ranks = preview.ranks;
  const standings = preview.standings!;
  const sources: Record<string, LeagueSource> = {
    "ranks-loading": over({ ranks: { rows: [], state: "loading" } }),
    "ranks-failed": over({
      ranks: { rows: [], state: "failed" },
      notice: "Couldn’t load your ranks.",
    }),
    "ranks-refresh-failed": over({
      ranks: { rows: ranks.rows.slice(0, 2), state: "failed" },
      notice: "Couldn’t refresh your ranks.",
    }),
    "all-new": over({
      ranks: {
        rows: ranks.rows.map((row) => ({
          ...row,
          rating: 1000,
          record: { wins: 0, losses: 0, draws: 0 },
        })),
        state: "ready",
      },
    }),
    "many-formats": over({
      formats: MORE_FORMATS,
      ranks: {
        rows: [
          ...ranks.rows,
          ...MORE_FORMATS.slice(PREVIEW_FORMATS.length).map((entry, index) => ({
            format: entry,
            rating: 1100 + index * 140,
            record: { wins: 6 + index, losses: 4, draws: index % 2 },
          })),
        ],
        state: "ready",
      },
    }),
    "standings-loading": over({
      standings: { ...standings, rows: [], state: "loading" },
    }),
    "standings-failed": over({
      standings: { ...standings, rows: [], state: "failed" },
      notice: "Couldn’t load standings.",
    }),
    "standings-refresh-failed": over({
      standings: { ...standings, state: "failed" },
      notice: "Couldn’t refresh standings.",
    }),
    "standings-appending": over({
      standings: {
        ...standings,
        isAppending: true,
        hasMore: true,
        loadMore: noop,
      },
    }),
    "append-failed": over({
      standings: { ...standings, hasMore: false, loadMore: noop },
      notice: "Couldn’t load more standings.",
    }),
    "own-unavailable": over({ standings: { ...standings, own: null } }),
  };
  return show(sources[state] ?? preview);
}
