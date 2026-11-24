import { useMemo, useState } from "react";
import type { DailyQuest, QuestsSource } from "./quest-sources";

const QUESTS: readonly DailyQuest[] = [
  {
    id: "preview-quest-1",
    text: "Cast 20 spells",
    progress: 12,
    target: 20,
    rewardCoins: 500,
    done: false,
  },
  {
    id: "preview-quest-2",
    text: "Play 3 games",
    progress: 3,
    target: 3,
    rewardCoins: 500,
    done: true,
  },
  {
    id: "preview-quest-3",
    text: "Play 25 lands",
    progress: 4,
    target: 25,
    rewardCoins: 750,
    done: false,
  },
];
const SWAPPED: DailyQuest = {
  id: "preview-quest-4",
  text: "Attack with 15 creatures",
  progress: 0,
  target: 15,
  rewardCoins: 500,
  done: false,
};

/** The preview's daily quests: three, one finished, with today's swap unused. */
export function useDemoQuests(): QuestsSource {
  const [swapped, setSwapped] = useState<string | null>(null);
  return useMemo(
    () => ({
      status: "ready",
      quests: QUESTS.map((quest) => (quest.id === swapped ? SWAPPED : quest)),
      canReroll: swapped === null,
      rerolling: null,
      reroll: setSwapped,
      wins: { count: 2, coins: 200 },
    }),
    [swapped],
  );
}
