import { useCallback, useEffect, useMemo, useState } from "react";
import { useBuilderCatalogue } from "../data/builder-catalogue";
import { useDemoIdentity } from "../account/demo-account";
import {
  competitionPlaces,
  seasonOf,
  STARTING_RATING,
  type FormatRank,
  type LeagueFormat,
  type LeagueSource,
  type MatchRecord,
  type PortraitData,
  type StandingRow,
} from "./league-sources";

/**
 * The local preview of League: this season's ranks in six formats and each
 * format's standings, paged fifty at a time as an account's would be. The
 * named leaders are the preview's public players, so their names open their
 * preview profiles; "You" opens your own.
 */
const PAGE = 50;
const APPEND_MS = 700;
const CENTRED = { x: 0.5, y: 0.5, scale: 1 };

export const PREVIEW_FORMATS: LeagueFormat[] = [
  "Modern",
  "Standard",
  "Commander",
  "Legacy",
  "Pioneer",
  "Premodern",
].map((name) => ({ id: `preview-format-${name.toLowerCase()}`, name }));
const formatId = (name: string) => `preview-format-${name.toLowerCase()}`;

const record = (wins: number, losses: number, draws = 0): MatchRecord => ({
  wins,
  losses,
  draws,
});
/** Your season, matching your preview profile. */
const OWN: Record<string, { rating: number; record: MatchRecord }> = {
  Modern: { rating: 1240, record: record(12, 8) },
  Standard: { rating: 1420, record: record(18, 12) },
  Commander: { rating: 1040, record: record(4, 6) },
  Legacy: { rating: 1820, record: record(24, 16) },
  Premodern: { rating: STARTING_RATING, record: record(0, 0) },
  // A rating carried over from last season, before any match this season.
  Pioneer: { rating: 1120, record: record(0, 0) },
};

/** The preview's public players, at the ratings and records their profiles show. */
type Named = [
  id: string,
  name: string,
  art: string,
  rating: number,
  record: MatchRecord,
];
const EMBER = ["emberfox", "EmberFox", "Goblin Guide"] as const;
const MOSS = ["mosswitch", "Mosswitch", "Aether Vial"] as const;
const ARCHIVIST = [
  "archivist",
  "Grand_Archivist_of_the_Ninth",
  "Goblin Matron",
] as const;
const NAMED: Record<string, Named[]> = {
  Modern: [
    [...EMBER, 2216, record(40, 12)],
    [...MOSS, 2132, record(35, 15)],
    [...ARCHIVIST, 1544, record(21, 17)],
  ],
  Standard: [
    [...MOSS, 1688, record(27, 14, 1)],
    [...EMBER, 1420, record(18, 12)],
  ],
  Commander: [
    [...MOSS, 1502, record(22, 16)],
    [...EMBER, 1240, record(12, 8)],
  ],
  Legacy: [[...MOSS, 1384, record(14, 11)]],
  Pioneer: [[...MOSS, 1310, record(11, 9, 2)]],
};
const FILLER = [
  "Aster",
  "Fern",
  "Slate",
  "Rook",
  "Wren",
  "Cinder",
  "Marrow",
  "Thistle",
  "Quill",
  "Brine",
  "Larkspur",
  "Ember_Veil",
  "Hollow",
  "Sable",
  "Juniper",
  "Vesper",
  "Tallow",
  "Moth",
  "Gravel",
  "Ochre",
];
/** How many play each format, the leader's rating and how fast ratings fall. */
const SHAPE: Record<string, { players: number; top: number; step: number }> = {
  Modern: { players: 140, top: 2190, step: 11 },
  Standard: { players: 60, top: 2240, step: 14 },
  Commander: { players: 96, top: 1990, step: 10 },
  Legacy: { players: 40, top: 2300, step: 12 },
  Pioneer: { players: 24, top: 1810, step: 25 },
  Premodern: { players: 0, top: 0, step: 0 },
};

type Entry = Omit<StandingRow, "place" | "portrait"> & { art: string | null };
/** Every participant of a format, best first; yours sits where your rating puts it. */
function entriesOf(format: string): Entry[] {
  const shape = SHAPE[format]!;
  const own = OWN[format]!;
  const filler: Entry[] = Array.from({ length: shape.players }, (_, index) => {
    // Standard's top two are tied, to show shared places.
    const rating =
      format === "Standard" && index < 2
        ? shape.top
        : shape.top - index * shape.step - (index % 3);
    const name = FILLER[index % FILLER.length]!;
    const wins = Math.max(1, Math.round((rating - 900) / 30));
    return {
      playerId: `preview-player-${format.toLowerCase()}-${index}`,
      name: index < FILLER.length ? name : `${name}${index}`,
      art: null,
      rating,
      record: record(
        wins,
        Math.max(1, Math.round(wins * 0.45)),
        index % 17 === 5 ? 1 : 0,
      ),
      isYou: false,
    };
  });
  const named: Entry[] = (NAMED[format] ?? []).map(
    ([playerId, name, art, rating, facts]) => ({
      playerId,
      name,
      art,
      rating,
      record: facts,
      isYou: false,
    }),
  );
  const played = own.record.wins + own.record.losses + own.record.draws > 0;
  const you: Entry[] = played
    ? [
        {
          playerId: "me",
          name: "You",
          art: null,
          rating: own.rating,
          record: own.record,
          isYou: true,
        },
      ]
    : [];
  // Best first; within a tie, the stable public identity decides, never the name.
  return [...filler, ...named, ...you].sort(
    (a, b) =>
      b.rating - a.rating ||
      (a.playerId < b.playerId ? -1 : a.playerId > b.playerId ? 1 : 0),
  );
}

/** Preview League. `format` is the Standings tab asked for, `now` the clock seasons follow. */
export function usePreviewLeague(
  format: string | null,
  now = new Date(),
): LeagueSource {
  const { catalogue, hasFailed } = useBuilderCatalogue();
  const [loaded, setLoaded] = useState<Record<string, number>>({});
  const [appending, setAppending] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => {
    if (!appending) return;
    const timer = window.setTimeout(() => {
      setLoaded((current) => ({
        ...current,
        [appending]: (current[appending] ?? PAGE) + PAGE,
      }));
      setAppending(null);
    }, APPEND_MS);
    return () => window.clearTimeout(timer);
  }, [appending]);

  const portraitOf = useCallback(
    (art: string | null, index: number): PortraitData | null => {
      const cards = catalogue?.cards ?? [];
      const card = art
        ? cards.find((entry) => entry.name === art)
        : cards[(index * 7) % Math.max(1, cards.length)];
      return card ? { artUrl: card.artUrl, crop: CENTRED } : null;
    },
    [catalogue],
  );
  const ownPortrait = useDemoIdentity().portrait;
  const selected = PREVIEW_FORMATS.find((entry) => entry.id === format) ?? null;
  const standings = useMemo(() => {
    if (!selected) return null;
    const entries = entriesOf(selected.name);
    const places = competitionPlaces(entries.map((entry) => entry.rating));
    const rows: StandingRow[] = entries.map((entry, index) => ({
      playerId: entry.playerId,
      name: entry.name,
      portrait: entry.isYou ? ownPortrait : portraitOf(entry.art, index),
      place: places[index]!,
      rating: entry.rating,
      record: entry.record,
      isYou: entry.isYou,
    }));
    return { rows, selected };
  }, [selected, portraitOf, ownPortrait]);

  if (!catalogue)
    return hasFailed ? { status: "unavailable" } : { status: "loading" };
  const ranks: FormatRank[] = PREVIEW_FORMATS.map((entry) => ({
    format: entry,
    rating: OWN[entry.name]!.rating,
    record: OWN[entry.name]!.record,
  }));
  const shown = standings ? (loaded[standings.selected.id] ?? PAGE) : PAGE;
  const ownStanding = standings
    ? (standings.rows.find((row) => row.isYou) ?? null)
    : null;
  const ownFacts = selected ? OWN[selected.name]! : null;
  return {
    status: "ready",
    season: seasonOf(now),
    formats: PREVIEW_FORMATS,
    ranks: { rows: ranks, state: "ready" },
    standings:
      standings && selected
        ? {
            format: selected,
            rows: standings.rows.slice(0, shown),
            state: "ready",
            own: ownStanding ?? {
              playerId: "me",
              name: "You",
              portrait: ownPortrait,
              place: null,
              rating: ownFacts!.rating,
              record: ownFacts!.record,
              isYou: true,
            },
            isAppending: appending === selected.id,
            hasMore: shown < standings.rows.length,
            loadMore: () => {
              if (appending || shown >= standings.rows.length) return;
              setAppending(selected.id);
            },
          }
        : null,
    notice,
    dismissNotice: () => setNotice(null),
  };
}

/** Exported for the rules tests. */
export const previewEntriesOf = (format: string) =>
  entriesOf(format).map(({ name, rating, isYou }) => ({ name, rating, isYou }));
export { formatId as previewFormatId };
