/** Season data for the local League walkthrough. Each row feeds both public views. */
export type SeasonRank = {
  format: string;
  tier: number;
  rating: number;
  wins: number;
  losses: number;
  draws?: number;
};

const riverglass = "RiverglassOfTheDistantNorth";
const season: Record<string, readonly SeasonRank[]> = {
  EmberFox: [
    { format: "Modern", tier: 5, rating: 2216, wins: 40, losses: 12 },
    { format: "Standard", tier: 2, rating: 1420, wins: 18, losses: 12 },
    { format: "Commander", tier: 1, rating: 1240, wins: 12, losses: 8 },
  ],
  Rook: [{ format: "Modern", tier: 5, rating: 2180, wins: 38, losses: 14 }],
  Mosswitch: [
    { format: "Modern", tier: 5, rating: 2132, wins: 35, losses: 15 },
  ],
  Aster: [{ format: "Modern", tier: 1, rating: 1260, wins: 13, losses: 8 }],
  Fern: [{ format: "Modern", tier: 1, rating: 1250, wins: 12, losses: 9 }],
  Slate: [{ format: "Modern", tier: 1, rating: 1230, wins: 11, losses: 8 }],
  Dawnkeeper: [
    { format: "Standard", tier: 5, rating: 2240, wins: 40, losses: 12 },
  ],
  [riverglass]: [
    { format: "Standard", tier: 5, rating: 2240, wins: 38, losses: 14 },
    {
      format: "Commander",
      tier: 1,
      rating: 1240,
      wins: 12,
      losses: 7,
      draws: 1,
    },
    { format: "Legacy", tier: 0, rating: 1000, wins: 0, losses: 0 },
  ],
  You: [
    { format: "Modern", tier: 1, rating: 1240, wins: 12, losses: 8 },
    { format: "Standard", tier: 2, rating: 1420, wins: 18, losses: 12 },
    { format: "Commander", tier: 0, rating: 1040, wins: 6, losses: 6 },
    { format: "Legacy", tier: 4, rating: 1820, wins: 20, losses: 5 },
    { format: "Vintage", tier: 2, rating: 1360, wins: 7, losses: 5 },
    { format: "Pioneer", tier: 1, rating: 1180, wins: 5, losses: 2 },
  ],
};
export const formats = [
  "Modern",
  "Standard",
  "Commander",
  "Legacy",
  "Vintage",
  "Pioneer",
];
export const longPlayer = riverglass;

export function ranksFor(name: string) {
  return Object.hasOwn(season, name) ? season[name]! : [];
}

export function recordFor(rank: SeasonRank, compact = false) {
  if (!rank.wins && !rank.losses && !rank.draws) return "No matches";
  if (compact)
    return [rank.wins, rank.losses, ...(rank.draws ? [rank.draws] : [])].join(
      "–",
    );
  return [
    `${rank.wins} ${rank.wins === 1 ? "win" : "wins"}`,
    `${rank.losses} ${rank.losses === 1 ? "loss" : "losses"}`,
    ...(rank.draws
      ? [`${rank.draws} ${rank.draws === 1 ? "draw" : "draws"}`]
      : []),
  ].join(" · ");
}

export function standingsFor(format: string, nearby: boolean) {
  // Modern and Standard show excerpts, including the viewer’s actual fixture place.
  const places: Record<string, number> | undefined =
    format === "Modern"
      ? nearby
        ? { Aster: 85, Fern: 86, You: 87, Slate: 88 }
        : { EmberFox: 1, Rook: 2, Mosswitch: 3, You: 87 }
      : format === "Standard"
        ? { Dawnkeeper: 1, [riverglass]: 1, You: 3 }
        : undefined;
  const ranked = Object.entries(season)
    .flatMap(([name, ranks]) => {
      const rank = ranks.find((r) => r.format === format);
      if (
        !rank ||
        !(rank.wins || rank.losses || rank.draws) ||
        (places && !Object.hasOwn(places, name))
      )
        return [];
      return [{ name, rank }];
    })
    .sort((a, b) => b.rank.rating - a.rank.rating);
  return ranked.map(({ name, rank }) => ({
    name,
    rank,
    place:
      places?.[name] ??
      1 + ranked.findIndex((row) => row.rank.rating === rank.rating),
  }));
}

export type StandingsLocation = {
  screen: string;
  format: string;
  tableTop: number;
  tabsLeft: number;
  player?: string;
  panel: string;
};

export function readStandingsLocation(
  value: unknown,
): StandingsLocation | undefined {
  if (
    !value ||
    typeof value !== "object" ||
    !(
      "screen" in value &&
      typeof value.screen === "string" &&
      /^ST-0[1-5]$/.test(value.screen)
    ) ||
    !(
      "format" in value &&
      typeof value.format === "string" &&
      formats.includes(value.format)
    ) ||
    !(
      "tableTop" in value &&
      typeof value.tableTop === "number" &&
      Number.isFinite(value.tableTop)
    ) ||
    !(
      "tabsLeft" in value &&
      typeof value.tabsLeft === "number" &&
      Number.isFinite(value.tabsLeft)
    ) ||
    !("panel" in value && typeof value.panel === "string")
  )
    return;
  return {
    screen: value.screen,
    format: value.format,
    tableTop: value.tableTop,
    tabsLeft: value.tabsLeft,
    panel: value.panel,
    ...("player" in value &&
    typeof value.player === "string" &&
    Object.hasOwn(season, value.player)
      ? { player: value.player }
      : {}),
  };
}
