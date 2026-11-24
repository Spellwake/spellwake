import type { SourceGap } from "../decks/deck-sources";

/** One of today's quests, as its card shows it. */
export type DailyQuest = {
  id: string;
  /** What to do, as a short sentence: "Cast 20 spells". */
  text: string;
  progress: number;
  target: number;
  rewardCoins: number;
  done: boolean;
};

export type QuestsSource =
  | { status: SourceGap }
  | {
      status: "ready";
      quests: readonly DailyQuest[];
      /** Whether today's one swap is still unused. */
      canReroll: boolean;
      /** The quest being swapped, while that is in flight. */
      rerolling: string | null;
      /** Swaps one unfinished quest for another. */
      reroll: (questId: string) => void;
      /** Games won today and the coins they have paid; null where that is not read. */
      wins: { count: number; coins: number } | null;
    };
