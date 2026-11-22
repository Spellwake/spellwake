/**
 * League: this season's rank in every ranked format, and each format's
 * standings. The shape both sources supply and the pure rules the screens
 * follow. Ratings are whole numbers; tiers follow the numeric bounds below.
 */

export const TIERS = [
  { name: "Bronze", lower: 0, reward: 0 },
  { name: "Silver", lower: 1100, reward: 250 },
  { name: "Gold", lower: 1300, reward: 500 },
  { name: "Platinum", lower: 1500, reward: 750 },
  { name: "Diamond", lower: 1700, reward: 1000 },
  { name: "Mythic", lower: 1900, reward: 1500 },
] as const;
export type TierName = (typeof TIERS)[number]["name"];
/** The rating every format starts the season with, before any match. */
export const STARTING_RATING = 1000;

export const tierOf = (rating: number) =>
  [...TIERS].reverse().find((tier) => rating >= tier.lower) ?? TIERS[0];
export const emblemUrl = (tier: TierName): string =>
  `${import.meta.env.BASE_URL}ranks/${tier.toLowerCase()}.svg`;

/** Progress to the next tier: the bounds the meter spans and its words. */
export function progressOf(rating: number):
  | { highest: true; label: string }
  | {
      highest: false;
      lowerBound: number;
      nextBound: number;
      label: string;
    } {
  const at = TIERS.findIndex((tier) => tier.name === tierOf(rating).name);
  const next = TIERS[at + 1];
  if (!next) return { highest: true, label: "Highest rank" };
  return {
    highest: false,
    lowerBound: TIERS[at]!.lower,
    nextBound: next.lower,
    label: `${next.lower - rating} to ${next.name}`,
  };
}

export interface MatchRecord {
  wins: number;
  losses: number;
  draws: number;
}
export const playedOf = (record: MatchRecord): number =>
  record.wins + record.losses + record.draws;
const count = new Intl.NumberFormat("en-US");
const plural = (amount: number, one: string, many: string) =>
  `${count.format(amount)} ${amount === 1 ? one : many}`;
/**
 * A season record is null when matches were played but the source cannot
 * say how many were won, lost or drawn; it is shown as unavailable, never
 * guessed.
 */
export type SeasonRecord = MatchRecord | null;
/** "12 wins · 7 losses · 1 draw", No matches, or Record unavailable. */
export function recordLong(record: SeasonRecord): string {
  if (record === null) return "Record unavailable";
  if (playedOf(record) === 0) return "No matches";
  return [
    plural(record.wins, "win", "wins"),
    plural(record.losses, "loss", "losses"),
    ...(record.draws > 0 ? [plural(record.draws, "draw", "draws")] : []),
  ].join(" · ");
}
/** "40–12", with draws as a third count when there are any. */
export function recordShort(record: SeasonRecord): string {
  if (record === null) return "—";
  if (playedOf(record) === 0) return "No matches";
  return [record.wins, record.losses, ...(record.draws ? [record.draws] : [])]
    .map((value) => count.format(value))
    .join("–");
}
export const ratingText = (rating: number): string => count.format(rating);

/** The season reward a participant's current tier would earn; none before playing. */
export const rewardOf = (
  rating: number,
  record: SeasonRecord,
): number | null =>
  record !== null && playedOf(record) === 0 ? null : tierOf(rating).reward;

/** Competition places: equal ratings share a place, and the next skips (1, 1, 3). */
export function competitionPlaces(ratings: readonly number[]): number[] {
  return ratings.map((rating, index) => {
    let first = index;
    while (first > 0 && ratings[first - 1] === rating) first -= 1;
    return first + 1;
  });
}

const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];
/** The UTC quarter a moment falls in: "Oct–Dec 2026", ending "Jan 1". */
export function seasonOf(now: Date): { label: string; ends: string } {
  const first = Math.floor(now.getUTCMonth() / 3) * 3;
  const end = new Date(Date.UTC(now.getUTCFullYear(), first + 3, 1));
  return {
    label: `${MONTHS[first]}–${MONTHS[first + 2]} ${now.getUTCFullYear()}`,
    ends: `${MONTHS[end.getUTCMonth()]} 1`,
  };
}

export interface LeagueFormat {
  id: string;
  name: string;
}
/** This season in one format, for the viewer. */
export interface FormatRank {
  format: LeagueFormat;
  rating: number;
  record: SeasonRecord;
}
export interface PortraitData {
  artUrl: string;
  crop: { x: number; y: number; scale: number };
}
export interface StandingRow {
  playerId: string;
  name: string;
  portrait: PortraitData | null;
  place: number;
  rating: number;
  record: SeasonRecord;
  isYou: boolean;
}
export interface Standings {
  format: LeagueFormat;
  rows: readonly StandingRow[];
  state: "loading" | "ready" | "failed";
  /** The viewer's own row: place null before playing; null itself when unreadable. */
  own: (Omit<StandingRow, "place"> & { place: number | null }) | null;
  isAppending: boolean;
  hasMore: boolean;
  loadMore: () => void;
}

export interface ReadyLeague {
  status: "ready";
  season: { label: string; ends: string };
  formats: readonly LeagueFormat[];
  ranks: { rows: readonly FormatRank[]; state: "loading" | "ready" | "failed" };
  /** Standings for the format asked for; null before a format is known. */
  standings: Standings | null;
  /** A read failed: the passive notice, which leaves by itself. */
  notice: string | null;
  dismissNotice: () => void;
}
export type LeagueSource =
  | { status: "loading" }
  | { status: "signed-out" }
  | { status: "unregistered" }
  | { status: "unavailable" }
  | ReadyLeague;
