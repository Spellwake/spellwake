import type { SourceGap } from "../decks/deck-sources";

export type SeasonFormatRank = {
  formatId: string;
  formatName: string;
  tier: string;
  emblemUrl: string;
  rating: number;
  lowerBound: number;
  nextBound: number;
  isHighest: boolean;
  progressLabel: string;
  wins: number;
  losses: number;
  draws: number;
};

export type SeasonQuest = {
  id: string;
  title: string;
  progress: number;
  target: number;
  rewardCoins: number;
  canReroll: boolean;
};

export type RankedDeckChoice = {
  id: string;
  name: string;
  formatId: string;
  formatName: string;
  revisionId: string | null;
};

export type ActiveQueue = {
  entryId: string;
  formatName: string;
  state: string;
};

export type SeasonSource =
  | { status: SourceGap }
  | {
      status: "ready";
      seasonName: string | null;
      seasonDates: string | null;
      ranks: readonly SeasonFormatRank[];
      quests: readonly SeasonQuest[];
      decks: readonly RankedDeckChoice[];
      formats: readonly { id: string; name: string }[];
      activeQueue: ActiveQueue | null;
      queueBusy: boolean;
      rerollingQuestId: string | null;
      notice: string | null;
      dismissNotice: () => void;
      joinQueue: (formatId: string, deckId: string) => Promise<void>;
      cancelQueue: () => Promise<void>;
      rerollQuest: (questId: string) => Promise<void>;
    };

export type StandingRow = {
  rankPosition: number;
  accountId: string;
  username: string;
  tier: string;
  emblemUrl: string;
  rating: number;
  wins: number;
  losses: number;
  draws: number;
  supporter: boolean;
};

export type StandingsSource =
  | { status: SourceGap }
  | {
      status: "ready";
      formats: readonly { id: string; name: string }[];
      selectedFormatId: string;
      onSelectFormat: (formatId: string) => void;
      seasonName: string | null;
      rows: readonly StandingRow[];
      total: number;
      isLoadingRows: boolean;
      error: string | null;
      onOpenProfile: (accountId: string) => void;
    };
