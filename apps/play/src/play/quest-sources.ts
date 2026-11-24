import type { SourceGap } from "../decks/deck-sources";

/** One of today's quests, as its card shows it. */
export type DailyQuest = {
  id: string;
  /** What to do, as a short sentence: "Cast 20 red or green spells". */
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
      /** The quest today's one swap may be spent on; null once it is used or cannot be. */
      swappable: string | null;
      /** True while a swap is in flight. */
      swapping: boolean;
      /** Swaps the swappable quest for another. */
      swap: () => void;
      /** Why the last swap did not happen; cleared by the next. */
      notice: string | null;
      /** Games won today and the coins they have paid; null where that is not read. */
      wins: { count: number; coins: number } | null;
    };
