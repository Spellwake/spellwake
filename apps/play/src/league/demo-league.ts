import { useState } from "react";
import { parseAsString, useQueryState } from "nuqs";
import type { SeasonSource, StandingsSource } from "./league-sources";

const DEMO_FORMATS = [
  { id: "modern", name: "Modern" },
  { id: "standard", name: "Standard" },
  { id: "commander", name: "Commander" },
] as const;

const emblem = (slug: string) => `${import.meta.env.BASE_URL}ranks/${slug}.svg`;

export function useDemoSeason(): SeasonSource {
  const [queued, setQueued] = useState(false);
  return {
    status: "ready",
    seasonName: "Autumn Season 2026",
    seasonDates: "Oct 1, 2026 – Dec 31, 2026",
    ranks: [
      {
        formatId: "modern",
        formatName: "Modern",
        tier: "Gold",
        emblemUrl: emblem("gold"),
        rating: 1420,
        lowerBound: 1400,
        nextBound: 1600,
        isHighest: false,
        progressLabel: "1420 / 1600",
        wins: 18,
        losses: 12,
        draws: 0,
      },
      {
        formatId: "standard",
        formatName: "Standard",
        tier: "Silver",
        emblemUrl: emblem("silver"),
        rating: 1240,
        lowerBound: 1200,
        nextBound: 1400,
        isHighest: false,
        progressLabel: "1240 / 1400",
        wins: 12,
        losses: 8,
        draws: 1,
      },
    ],
    quests: [
      {
        id: "demo-q1",
        title: "Cast 20 red or green spells",
        progress: 14,
        target: 20,
        rewardCoins: 500,
        canReroll: true,
      },
      {
        id: "demo-q2",
        title: "Play 3 matches in any format",
        progress: 2,
        target: 3,
        rewardCoins: 750,
        canReroll: true,
      },
    ],
    decks: [
      {
        id: "demo-goblins",
        name: "Goblins",
        formatId: "modern",
        formatName: "Modern",
        revisionId: "demo-rev-1",
      },
    ],
    formats: [...DEMO_FORMATS],
    activeQueue: queued
      ? { entryId: "demo-q", formatName: "Modern", state: "searching" }
      : null,
    queueBusy: false,
    rerollingQuestId: null,
    notice: null,
    dismissNotice: () => undefined,
    joinQueue: async () => setQueued(true),
    cancelQueue: async () => setQueued(false),
    rerollQuest: async () => undefined,
  };
}

export function useDemoStandings(): StandingsSource {
  const [formatId, setFormatId] = useState("modern");
  const [, setPlayer] = useQueryState("player", parseAsString);
  return {
    status: "ready",
    formats: [...DEMO_FORMATS],
    selectedFormatId: formatId,
    onSelectFormat: setFormatId,
    seasonName: "Autumn Season 2026",
    rows: [
      {
        rankPosition: 1,
        accountId: "emberfox",
        username: "EmberFox",
        tier: "Mythic",
        emblemUrl: emblem("mythic"),
        rating: 2216,
        wins: 40,
        losses: 12,
        draws: 0,
        supporter: true,
      },
      {
        rankPosition: 2,
        accountId: "mosswitch",
        username: "Mosswitch",
        tier: "Mythic",
        emblemUrl: emblem("mythic"),
        rating: 2132,
        wins: 35,
        losses: 15,
        draws: 1,
        supporter: false,
      },
      {
        rankPosition: 3,
        accountId: "archivist",
        username: "Grand_Archivist_of_the_Ninth",
        tier: "Platinum",
        emblemUrl: emblem("platinum"),
        rating: 1544,
        wins: 21,
        losses: 17,
        draws: 0,
        supporter: false,
      },
    ],
    total: 3,
    isLoadingRows: false,
    error: null,
    onOpenProfile: (id) => void setPlayer(id),
  };
}
