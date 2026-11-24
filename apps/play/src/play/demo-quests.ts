import { useMemo, useState } from "react";
import type { DailyQuest, QuestsSource } from "./quest-sources";

const QUESTS: readonly DailyQuest[] = [
  {
    id: "preview-quest-1",
    text: "Cast 20 red or green spells",
    progress: 12,
    target: 20,
    rewardCoins: 500,
    done: false,
  },
  {
    id: "preview-quest-2",
    text: "Cast 15 black or green spells",
    progress: 15,
    target: 15,
    rewardCoins: 500,
    done: true,
  },
  {
    id: "preview-quest-3",
    text: "Cast 30 red or green spells",
    progress: 4,
    target: 30,
    rewardCoins: 750,
    done: false,
  },
];
const SWAPPED: DailyQuest = {
  id: "preview-quest-4",
  text: "Cast 20 black or green spells",
  progress: 0,
  target: 20,
  rewardCoins: 500,
  done: false,
};

/** How the preview's quests behave, for review and the stress checks. */
export type DemoQuests =
  /** Three quests, one finished, today's swap unused. */
  | "three"
  /** One quest and nothing won yet. */
  | "one"
  /** No quests: the row is absent. */
  | "none"
  /** The swap is refused. */
  | "refused";

/** The preview's stand-in for the account's daily quests. */
export function useDemoQuests(behaviour: DemoQuests = "three"): QuestsSource {
  const [swapped, setSwapped] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  return useMemo(() => {
    const quests =
      behaviour === "none"
        ? []
        : behaviour === "one"
          ? QUESTS.slice(0, 1)
          : QUESTS.map((quest, index) =>
              index === 0 && swapped ? SWAPPED : quest,
            );
    return {
      status: "ready",
      quests,
      swappable: swapped || quests.length === 0 ? null : quests[0]!.id,
      swapping: false,
      swap: () => {
        if (behaviour === "refused")
          setNotice("No other quest is available today.");
        else setSwapped(true);
      },
      notice,
      wins:
        behaviour === "one" ? { count: 0, coins: 0 } : { count: 2, coins: 200 },
    };
  }, [behaviour, swapped, notice]);
}
